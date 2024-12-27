{ config, lib, pkgs, ... }:
let
  cfg = config.services.zstdp;
in

with lib;

{
  options.services.zstdp = {
    enable = mkEnableOption "Enable zstd proxy";
    package = mkOption {
      type = types.package;
      default = pkgs.zstdp;
    };
    services = let
      bindOptions = types.submodule ({ ... }: {
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
            example = "8766";
          };
        };
      });
      serviceModule = types.submodule ({ ... }: {
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
            default = [];
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
      });
    in mkOption {
      type = types.attrsOf serviceModule;
      default = {};
    };
  };

  config = mkIf cfg.enable {
    assertions = let
      onlyOneMode = mapAttrsToList
        (svcName: svcConfig: {
          assertion = svcConfig.forward != null && svcConfig.serve == null || svcConfig.forward == null && svcConfig.serve != null;
          message = ''zstdp service "${svcName}" must have ONE and ONLY ONE of `forward` and `serve` configured!'';
        })
        cfg.services;
      mustSpecifyIfSPAInFileMode = mapAttrsToList
        (svcName: svcConfig: {
          assertion = svcConfig.serve == null || svcConfig.spa != null;
          message = ''zstdp service "${svcName}" uses file-serving mode, it should set if or not it is an SPA!'';
        })
        cfg.services;
    in
      onlyOneMode ++ mustSpecifyIfSPAInFileMode;

    environment.systemPackages = [ cfg.package ];

    systemd.services = let
      mkService = svcName: svcConfig: nameValuePair "zstdp-${svcName}" {
        description = "Zstd proxy for service '${svcName}'";
        documentation = [ "https://github.com/blurgyy/zstdp" ];
        after = [ "network-online.target" "network.target" ];
        path = [ cfg.package ];
        wantedBy = [ "multi-user.target" ];
        serviceConfig = {
          Restart = "on-failure";
          RestartSec = 5;
          DynamicUser = true;
        };
        script = ''
          zstdp -b ${svcConfig.bind.address} -p ${toString svcConfig.bind.port} \
            ${if svcConfig.forward != null
              then "-f ${svcConfig.forward}"
              else "-s ${svcConfig.serve}${optionalString svcConfig.spa " --spa"}"
            } \
            ${concatStringsSep
              " "
              (map
                (pathRegex: "-i ${escapeShellArg pathRegex}")
                svcConfig.bypassUris)
            } \
            -z ${toString svcConfig.zstdLevel} \
            -g ${toString svcConfig.gzipLevel}
        '';
      };
      mkServices = services: attrValues (mapAttrs mkService services);
    in listToAttrs (mkServices cfg.services);
  };
}
