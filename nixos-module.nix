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
      serviceModule = types.submodule ({ ... }: {
        options = {
          name = mkOption { type = types.str; };
          listen = mkOption {
            type = types.str;
            example = "127.0.0.1:32768";
          };
          forward = mkOption {
            type = types.str;
            example = "127.0.0.1:8080";
            description = ''
              Address to the server whose response will be compressed before sending back to the
              client.
            '';
          };
          zstdLevel = mkOption {
            type = types.int;
            default = 3;
          };
        };
      });
    in mkOption {
      type = types.attrsOf serviceModule;
      default = {};
    };
  };

  config = mkIf cfg.enable {

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
          zstdp -l ${svcConfig.listen} -f ${svcConfig.forward} -z ${toString svcConfig.zstdLevel}
        '';
      };
      mkServices = services: attrValues (mapAttrs mkService services);
    in listToAttrs (mkServices cfg.services);
  };
}
