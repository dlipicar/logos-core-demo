# The Windows app: the Slint UI and the headless client cross-built with MinGW
# on x86_64-linux, and a portable zip that carries them with the runtime, its
# hosts, the modules and every DLL they import.
{ lib, logos-nix, nixpkgs, rust-overlay, logos-rust-sdk, src }:

let
  buildSystem = "x86_64-linux";
  wpkgs = logos-nix.lib.mkWindowsPkgs { inherit buildSystem; };
  # Toolchains run on the build machine and target Windows.
  bpkgs = import nixpkgs {
    system = buildSystem;
    overlays = logos-nix.lib.nativeOverlays ++ [ (import rust-overlay) ];
  };
  triple = "x86_64-pc-windows-gnu";
  toolchain = bpkgs.rust-bin.stable."1.96.0".default.override { targets = [ triple ]; };
  rustPlatform = bpkgs.makeRustPlatform { cargo = toolchain; rustc = toolchain; };
  cc = wpkgs.stdenv.cc;
  objdump = "${cc.bintools.bintools}/bin/${cc.targetPrefix}objdump";

  # Where the DLLs the runtime, its hosts and the modules import live.
  dllDeps = with wpkgs; [
    openssl boost spdlog fmt libsodium zlib icu libblake3 zstd tbb
    stdenv.cc.cc windows.pthreads windows.mcfgthreads
  ];
  dllDirs = lib.unique (lib.concatMap (p: [ "${lib.getLib p}" "${lib.getBin p}" "${p}" ]) dllDeps);
in
{
  inherit wpkgs;

  # logos-core-demo.exe and logos-core-demo-headless.exe, linked against
  # liblogos' Windows lib output.
  programs = { liblogosLib }:
    let host = logos-rust-sdk.lib.hostBuildSupportWindows { inherit liblogosLib; windowsPkgs = wpkgs; };
    in rustPlatform.buildRustPackage {
      pname = "logos-core-demo-windows";
      version = "0.1.0";
      inherit src;
      cargoLock = { lockFile = src + "/Cargo.lock"; allowBuiltinFetchGit = true; };
      inherit (host) nativeBuildInputs env;
      # The stdenv here is the build machine's; drive cargo for the target.
      buildPhase = ''
        runHook preBuild
        export CARGO_HOME=$TMPDIR/cargo
        cargo build --release --offline --target ${triple} \
          -p logos-core-demo -p logos-core-demo-headless \
          --bin logos-core-demo --bin logos-core-demo-headless
        runHook postBuild
      '';
      installPhase = ''
        runHook preInstall
        mkdir -p $out/bin
        cp target/${triple}/release/logos-core-demo.exe target/${triple}/release/logos-core-demo-headless.exe $out/bin/
        runHook postInstall
      '';
      doCheck = false;
      dontFixup = true;
    };

  # A directory that runs as-is on Windows: bin/ holds every program and DLL,
  # modules/ the runtime's bundled modules and app-modules/ the app's own.
  portable = { programs, liblogos, hostRemote, bundledModules, appModules }:
    bpkgs.runCommand "logos-core-demo-windows" { nativeBuildInputs = [ bpkgs.python3 ]; } ''
      mkdir -p $out/bin $out/modules $out/app-modules
      cp -L ${programs}/bin/*.exe $out/bin/
      cp -L ${liblogos.logos-liblogos-bin}/bin/logos_runtime.exe ${liblogos.logos-liblogos-bin}/bin/logos_host_plain.exe $out/bin/
      cp -L ${hostRemote}/bin/logos_host_remote.exe $out/bin/
      cp -L ${liblogos.logos-liblogos-lib}/lib/*.dll $out/bin/
      ${lib.concatMapStrings (m: "cp -rL ${m}/modules/. $out/modules/\n") bundledModules}
      ${lib.concatMapStrings (m: "cp -rL ${m}/modules/. $out/app-modules/\n") appModules}
      chmod -R u+w $out
      python3 ${./windows-dlls.py} --objdump ${objdump} --into $out/bin \
        --search ${lib.concatStringsSep " " dllDirs} -- \
        $out/bin/*.exe $out/bin/*.dll $(find $out/modules $out/app-modules -name '*.dll')
    '';

  zip = portable: bpkgs.runCommand "logos-core-demo-windows-zip" { nativeBuildInputs = [ bpkgs.zip ]; } ''
    mkdir -p $out work
    cp -rL ${portable} work/logos-core-demo
    chmod -R u+w work
    (cd work && zip -qr $out/logos-core-demo-x86_64-windows.zip logos-core-demo)
  '';
}
