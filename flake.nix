{
  description = "Logos Core Demo: Qt-free apps that run a Logos runtime and use a node on a peered one";

  inputs = {
    logos-nix.url = "github:logos-co/logos-nix";
    nixpkgs.follows = "logos-nix/nixpkgs";
    # Builds the demo's modules; its protocol is the one the runtime speaks.
    logos-module-builder.url = "github:logos-co/logos-module-builder/feat/peering";
    # The runtime the apps spawn (logos_runtime, the plain hosts, the bundled
    # modules) and the daemon the node runs in.
    logos-logoscore-cli.url = "github:logos-co/logos-logoscore-cli/feat/standalone-apps";
    # Host embedding and the client generator.
    logos-rust-sdk.url = "github:logos-co/logos-rust-sdk/feat/standalone-apps";
    logos-rust-sdk.inputs.logos-nix.follows = "logos-nix";
  };

  outputs = { self, nixpkgs, logos-nix, logos-module-builder, logos-logoscore-cli, logos-rust-sdk }:
    let
      lib = nixpkgs.lib;
      systems = [ "aarch64-darwin" "x86_64-darwin" "aarch64-linux" "x86_64-linux" ];
      mkPkgs = system: import nixpkgs { inherit system; overlays = logos-nix.lib.nativeOverlays; };
      forAllSystems = f: lib.genAttrs systems (system: f { inherit system; pkgs = mkPkgs system; });

      mkModule = name: logos-module-builder.lib.mkLogosModule {
        src = ./modules/${name};
        configFile = ./modules/${name}/metadata.json;
      };
      modules = lib.genAttrs [ "hello_module" "bc_probe" "fake_blockchain" ] mkModule;

      # The contracts the apps call, by module name.
      contracts = system: {
        blockchain_module = ./modules/bc_probe/contracts/blockchain_module.lidl;
        hello_module = "${modules.hello_module.packages.${system}.lidl}/hello_module.lidl";
        bc_probe = "${modules.bc_probe.packages.${system}.lidl}/bc_probe.lidl";
        peering_module =
          "${logos-logoscore-cli.inputs.logos-peering.packages.${system}.peering_module-lidl}/peering_module.lidl";
      };

      # What an app ships to run its own runtime: no logosctl, no Qt host.
      runtimePayload = { pkgs, system }:
        let ctl = logos-logoscore-cli.packages.${system}.ctl; in
        pkgs.runCommand "logos-core-demo-runtime" { } ''
          mkdir -p $out/bin $out/lib $out/modules
          for b in logos_runtime logos_host_plain logos_host_remote; do
            cp -L ${ctl}/bin/$b $out/bin/
          done
          cp -L ${ctl}/lib/* $out/lib/
          cp -rL ${ctl}/modules/. $out/modules/
          chmod -R u+w $out
        '';
    in
    {
      packages = forAllSystems ({ pkgs, system }:
        {
          runtime = runtimePayload { inherit pkgs system; };
          # The generated clients, as committed under demo-core/src/clients.
          clients = logos-rust-sdk.lib.mkClients { inherit system; lidls = contracts system; };
          daemon = logos-logoscore-cli.packages.${system}.ctl;
        }
        // lib.concatMapAttrs (name: module: {
          ${name} = module.packages.${system}.install;
          "${name}-lidl" = module.packages.${system}.lidl;
        }) modules);

      checks = forAllSystems ({ pkgs, system }: {
        clients-up-to-date = logos-rust-sdk.lib.clientsUpToDate {
          inherit system;
          lidls = contracts system;
          committed = ./demo-core/src/clients;
        };
      });
    };
}
