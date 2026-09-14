{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.zstdp;
  # Per-service memory floor (MiB); see the serviceConfig note below.
  memoryMinMiB = 64;
in

with lib;

{
  options.services.zstdp = {
    enable = mkEnableOption "Enable zstd proxy";
    package = mkOption {
      type = types.package;
      default = pkgs.zstdp;
    };
    services =
      let
        bindOptions = types.submodule (
          { ... }:
          {
            options = {
              address = mkOption {
                type = types.str;
                description = "Address to bind to";
                default = "127.0.0.1";
                example = "0.0.0.0";
              };
              port = mkOption {
                type = types.int;
                description = "Port to listen for requests";
                example = 8766;
              };
            };
          }
        );
        serviceModule = types.submodule (
          { ... }:
          {
            options = {
              name = mkOption { type = types.str; };
              bind = mkOption {
                type = bindOptions;
                description = "At which address and port will zstdp listen on";
              };
              forward = mkOption {
                type = types.nullOr types.str;
                default = null;
                example = "127.0.0.1:8080";
                description = ''
                  Address to the server whose response will be compressed before sending back to the
                  client.
                '';
              };
              serve = mkOption {
                type = types.nullOr types.str;
                default = null;
                example = "/var/lib/webapps/some_app";
                description = ''
                  Work in directory-serving mode and serve this directory, handling pre-compressions
                  (check for on-disk files with .zst or .gz extensions) and on-the-fly compressions if
                  client supports them.
                '';
              };
              bypassUris = mkOption {
                type = types.listOf types.str;
                default = [ ];
                example = [ "^/log/.*" ];
                description = "A list of regular expressions to directly return the backend's response.";
              };
              spa = mkOption {
                type = types.nullOr types.bool;
                example = null;
                description = ''
                  SPAs do routing on the client side and all non-file requests should be redirected to
                  `/index.html`.
                  In file-serving mode, enabling this serves the directory as an SPA.
                  Does nothing when in forwarding mode.
                '';
              };
              zstdLevel = mkOption {
                type = types.int;
                default = 3;
              };
              gzipLevel = mkOption {
                type = types.int;
                default = 6;
              };
            };
          }
        );
      in
      mkOption {
        type = types.attrsOf serviceModule;
        default = { };
      };
  };

  config = mkIf cfg.enable {
    assertions =
      let
        onlyOneMode = mapAttrsToList (svcName: svcConfig: {
          assertion =
            svcConfig.forward != null && svcConfig.serve == null
            || svcConfig.forward == null && svcConfig.serve != null;
          message = ''zstdp service "${svcName}" must have ONE and ONLY ONE of `forward` and `serve` configured!'';
        }) cfg.services;
        mustSpecifyIfSPAInFileMode = mapAttrsToList (svcName: svcConfig: {
          assertion = svcConfig.serve == null || svcConfig.spa != null;
          message = ''zstdp service "${svcName}" uses file-serving mode, it should set if or not it is an SPA!'';
        }) cfg.services;
      in
      onlyOneMode ++ mustSpecifyIfSPAInFileMode;

    environment.systemPackages = [ cfg.package ];

    # The top rung of each service's memory floor (see serviceConfig): one
    # slice for all zstdp services, protecting the sum of their floors.
    systemd.slices.zstdp = {
      description = "zstd proxies";
      sliceConfig.MemoryMin = "${toString (memoryMinMiB * length (attrNames cfg.services))}M";
    };

    systemd.services =
      let
        mkService =
          svcName: svcConfig:
          nameValuePair "zstdp-${svcName}" {
            description = "Zstd proxy for service '${svcName}'";
            documentation = [ "https://github.com/blurgyy/zstdp" ];
            after = [
              "network-online.target"
              "network.target"
            ];
            path = [ cfg.package ];
            wantedBy = [ "multi-user.target" ];
            serviceConfig = {
              Restart = "always";
              RestartSec = 5;
              DynamicUser = true;
              # A conservative resource floor for a proxy that sits in front of
              # something people are waiting on. `memory.min` is the RAM the
              # kernel will not reclaim from this unit while any unprotected
              # memory remains on the host, so a memory-hungry neighbour cannot
              # page the proxy out from under its clients (observed 2026-09-14:
              # a zstdp with 4 MiB resident and 84 MiB in swap, every request
              # paying the fault-in). Sized well above a proxy's steady state,
              # and a floor protects only what the unit actually uses. The
              # kernel bounds a unit's effective `memory.min` by every ancestor's,
              # so the unit lives in the slice below, whose own floor is the
              # sum over services; a direct child of the root is bounded by
              # nothing but itself, which completes the chain here.
              MemoryMin = "${toString memoryMinMiB}M";
              Slice = "zstdp.slice";
              # Below the default so the OOM killer (and earlyoom) shed the
              # memory hog before the proxy, but well above a control plane's
              # own shield (-900 and lower), so this never outranks the service
              # it fronts.
              OOMScoreAdjust = -500;
            };
            script = ''
              exec zstdp -b ${svcConfig.bind.address} -p ${toString svcConfig.bind.port} \
                ${
                  if svcConfig.forward != null then
                    "-f ${svcConfig.forward}"
                  else
                    "-s ${svcConfig.serve}${optionalString svcConfig.spa " --spa"}"
                } \
                ${concatStringsSep " " (map (pathRegex: "-i ${escapeShellArg pathRegex}") svcConfig.bypassUris)} \
                -z ${toString svcConfig.zstdLevel} \
                -g ${toString svcConfig.gzipLevel}
            '';
          };
        mkServices = services: attrValues (mapAttrs mkService services);
      in
      listToAttrs (mkServices cfg.services);
  };
}
