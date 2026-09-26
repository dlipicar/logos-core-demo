{
  description = "Logos Core Demo: Qt-free apps that run a Logos runtime and use a node on a peered one";

  inputs = {
    logos-nix.url = "github:logos-co/logos-nix/feat/standalone-apps";
    nixpkgs.follows = "logos-nix/nixpkgs";
    # Builds the demo's modules; its protocol is the one the runtime speaks.
    logos-module-builder.url = "github:logos-co/logos-module-builder/feat/standalone-apps";
    # The runtime the apps spawn (logos_runtime, the plain hosts, the bundled
    # modules) and the daemon the node runs in.
    logos-logoscore-cli.url = "github:logos-co/logos-logoscore-cli/feat/standalone-apps";
    # Host embedding and the client generator.
    logos-rust-sdk.url = "github:logos-co/logos-rust-sdk/feat/standalone-apps";
    logos-rust-sdk.inputs.logos-nix.follows = "logos-nix";
    # Slint needs a newer rustc than the nixpkgs pin ships.
    rust-overlay.follows = "logos-module-builder/rust-overlay";
    # The C++ client generator and LogosCore, for bc-watch.
    logos-cpp-sdk.url = "github:logos-co/logos-cpp-sdk/feat/standalone-apps";
  };

  outputs = { self, nixpkgs, logos-nix, logos-module-builder, logos-logoscore-cli, logos-rust-sdk, rust-overlay, logos-cpp-sdk }:
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

      # The contracts the apps call, by module name. The two from other repos
      # are vendored (a C++ build outside nix needs them) and drift-checked.
      contracts = system: {
        blockchain_module = ./modules/bc_probe/contracts/blockchain_module.lidl;
        hello_module = "${modules.hello_module.packages.${system}.lidl}/hello_module.lidl";
        bc_probe = "${modules.bc_probe.packages.${system}.lidl}/bc_probe.lidl";
        peering_module = ./contracts/peering_module.lidl;
      };
      publishedPeeringLidl = system:
        "${logos-logoscore-cli.inputs.logos-peering.packages.${system}.peering_module-lidl}/peering_module.lidl";

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
      rustPlatformFor = pkgs:
        let toolchain = (pkgs.extend (import rust-overlay)).rust-bin.stable."1.96.0".default;
        in pkgs.makeRustPlatform { cargo = toolchain; rustc = toolchain; };

      rustSrc = lib.fileset.toSource {
        root = ./.;
        fileset = lib.fileset.unions [ ./Cargo.toml ./Cargo.lock ./demo-core ./headless ./app ];
      };
      android = import ./nix/android.nix {
        inherit lib logos-nix nixpkgs rust-overlay;
        src = rustSrc;
      };
      # The stack's Android outputs, through the runtime's own pins.
      cli = logos-logoscore-cli.inputs;
      androidLiblogos = cli.logos-liblogos.packages.aarch64-android;

      # What winit and femtovg open at run time on Linux.
      linuxGuiLibs = pkgs: with pkgs; [
        libxkbcommon wayland libGL fontconfig freetype
        xorg.libX11 xorg.libXcursor xorg.libXi xorg.libXrandr
      ];

      # One crate of the workspace, linked against the runtime it will spawn.
      mkRustPackage = { pkgs, system, crate, bin, meta ? { } }:
        let runtime = self.packages.${system}.runtime; in
        (rustPlatformFor pkgs).buildRustPackage {
          pname = bin;
          version = "0.1.0";
          src = rustSrc;
          cargoLock = { lockFile = ./Cargo.lock; allowBuiltinFetchGit = true; };
          cargoBuildFlags = [ "-p" crate ];
          cargoTestFlags = [ "-p" "demo-core" ];
          env.LOGOS_HOST_LIB_DIR = "${runtime}/lib";
          nativeBuildInputs = [ pkgs.pkg-config ];
          buildInputs = lib.optionals pkgs.stdenv.isLinux (linuxGuiLibs pkgs);
          # The runtime and the demo's modules, where the app looks for them.
          postInstall = ''
            mkdir -p $out/share/logos-core-demo/modules
            ln -s ${runtime} $out/share/logos-core-demo/runtime
            cp -r ${modules.hello_module.packages.${system}.install}/modules/. $out/share/logos-core-demo/modules/
            cp -r ${modules.bc_probe.packages.${system}.install}/modules/. $out/share/logos-core-demo/modules/
          '';
          postFixup = lib.optionalString pkgs.stdenv.isLinux ''
            patchelf --add-rpath ${lib.makeLibraryPath (linuxGuiLibs pkgs)} $out/bin/${bin}
          '';
          inherit meta;
        };
    in
    {
      packages = forAllSystems ({ pkgs, system }:
        {
          runtime = runtimePayload { inherit pkgs system; };
          app = mkRustPackage { inherit pkgs system; crate = "logos-core-demo"; bin = "logos-core-demo";
                                meta.mainProgram = "logos-core-demo"; };
          headless = mkRustPackage { inherit pkgs system; crate = "logos-core-demo-headless";
                                     bin = "logos-core-demo-headless";
                                     meta.mainProgram = "logos-core-demo-headless"; };
          default = self.packages.${system}.app;
          # The C++ client: generated Qt-free clients over LogosCore.
          bc-watch = pkgs.stdenv.mkDerivation {
            pname = "bc-watch";
            version = "0.1.0";
            src = lib.fileset.toSource {
              root = ./.;
              fileset = lib.fileset.unions [ ./cpp ./contracts ./modules/bc_probe/contracts ];
            };
            sourceRoot = "source/cpp/bc-watch";
            nativeBuildInputs = [ pkgs.cmake pkgs.ninja ];
            buildInputs = [ logos-cpp-sdk.packages.${system}.default pkgs.nlohmann_json ];
            cmakeFlags = [
              "-DLOGOS_RUNTIME_LIB_DIR=${self.packages.${system}.runtime}/lib"
              "-DLOGOS_PROTOCOL_INCLUDE_DIR=${logos-cpp-sdk.inputs.logos-protocol.packages.${system}.logos-protocol-include}/include/cpp"
            ];
            meta.mainProgram = "bc-watch";
          };
          # The generated clients, as committed under demo-core/src/clients.
          clients = logos-rust-sdk.lib.mkClients { inherit system; lidls = contracts system; };
          daemon = logos-logoscore-cli.packages.${system}.ctl;
        }
        // lib.optionalAttrs (system == "aarch64-darwin") {
          android-emulator-sdk = android.emulatorSdk system;
        }
        // lib.concatMapAttrs (name: module: {
          ${name} = module.packages.${system}.install;
          "${name}-lidl" = module.packages.${system}.lidl;
        }) modules)
        // {
          # Built on the build system logos-nix names for Android.
          aarch64-android = rec {
            app-lib = android.appLib { hostLibDir = "${androidLiblogos.logos-liblogos-lib}/lib"; };
            apk =
              let
                a = android.apkgs;
                runtime = androidLiblogos.logos-liblogos-bin;
                peering = cli.logos-peering.packages.aarch64-android;
                bundled = name: version: { group = "bundled"; inherit name version; dir = "${runtime}/modules/${name}"; };
                fromPackage = group: name: pkg: { inherit group name; version = "0.1.0"; dir = "${pkg}/lib"; };
              in
              android.apk {
                appLib = app-lib;
                executables = {
                  logos_runtime = "${runtime}/bin/logos_runtime";
                  logos_host_plain = "${runtime}/bin/logos_host_plain";
                  logos_host_remote = "${peering.logos_host_remote}/bin/logos_host_remote";
                };
                libraries = [ ];
                modules = [
                  (bundled "capability_module" "1.0.0")
                  (bundled "modules_state" "0.1.0")
                  (fromPackage "bundled" "peering_identity" peering.peering_identity)
                  (fromPackage "bundled" "peering_module" peering.peering_module)
                  (fromPackage "app" "hello_module" modules.hello_module.packages.aarch64-android.default)
                  (fromPackage "app" "bc_probe" modules.bc_probe.packages.aarch64-android.default)
                ];
                searchPath = [ "${runtime}/lib" "${peering.libpeering}/lib" ]
                  ++ map (p: "${lib.getLib p}/lib") [ a.boost a.openssl a.spdlog a.fmt a.libsodium a.zstd a.libiconv a.libblake3 ];
              };
          };
        };

      checks = forAllSystems ({ pkgs, system }: {
        # Two runtimes on one machine: a daemon exports fake_blockchain and the
        # headless app links, imports, reads, probes, survives a restart, and is
        # refused where the peer grants nothing.
        e2e = pkgs.runCommand "logos-core-demo-e2e" {
          nativeBuildInputs = [ pkgs.bash pkgs.jq pkgs.python3 pkgs.coreutils ];
          CTL = "${self.packages.${system}.daemon}/bin/logosctl";
          HEADLESS = "${self.packages.${system}.headless}/bin/logos-core-demo-headless";
          FAKE = "${self.packages.${system}.fake_blockchain}";
          APP_HOME = "${self.packages.${system}.headless}/share/logos-core-demo";
        } ''
          export HOME=$TMPDIR/home
          mkdir -p $HOME
          bash ${./tests/e2e.sh} > $TMPDIR/e2e.log 2>&1 || { tail -80 $TMPDIR/e2e.log; exit 1; }
          grep -v '^\[20' $TMPDIR/e2e.log
          cp $TMPDIR/e2e.log $out
        '';
        contracts-up-to-date = pkgs.runCommand "logos-core-demo-contracts" { } ''
          diff -u ${publishedPeeringLidl system} ${./contracts/peering_module.lidl}
          touch $out
        '';
        clients-up-to-date = logos-rust-sdk.lib.clientsUpToDate {
          inherit system;
          lidls = contracts system;
          committed = ./demo-core/src/clients;
        };
      });
    };
}
