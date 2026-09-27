# The iOS Simulator app. iOS starts no process, so the runtime runs inside the
# app: its C++ stack is cross-built here with Xcode's clang (logos-nix's
# xcodeClang), the Slint app with rust-overlay's aarch64-apple-ios-sim, and a
# .app carries both with every module. Sources are the runtime's own pins.
{ lib, logos-nix, nixpkgs, rust-overlay, src
, liblogosFlake   # the runtime's logos-liblogos, whose inputs pin the rest
, moduleTrees     # [ { group; name; tree; native; builder; } ]: each module's `generate`
                  # output (native codegen) and its native `lib` build, whose flags it keeps
}:

let
  # The Xcode installed on the build machine; the wrapper refuses any other.
  xcodeVersion = "26.5";
  xcodeBuild = "17F42";
  minIos = "17.0";
  sdk = "iphonesimulator";
  triple = "aarch64-apple-ios-sim";

  ipkgs = logos-nix.lib.mkIosPkgs { inherit xcodeVersion xcodeBuild; };
  bpkgs = import nixpkgs {
    system = "aarch64-darwin";
    overlays = logos-nix.lib.nativeOverlays ++ [ (import rust-overlay) ];
  };
  deps = import ./ios-deps.nix { iosPkgs = ipkgs; inherit minIos; };
  inherit (ipkgs) xcodeWrapper;

  inputs = liblogosFlake.inputs;
  lgxSrc = inputs.logos-package-manager.inputs.logos-package;
  # Header-only, so the native packages serve every target.
  containerHeaders = inputs.logos-container.packages.aarch64-darwin.default;
  moduleLoaderHeaders = inputs.logos-module-loader.packages.aarch64-darwin.default;
  semver = import "${lgxSrc}/nix/cpp-semver.nix" { pkgs = bpkgs; };

  # A CMake project for the simulator on Xcode's clang. Never set preConfigure:
  # xcodeClang's sets the compilers and CMAKE_SYSTEM_NAME=iOS. No cc-wrapper puts
  # each input's include/ on the path, so the flags do; and iOS makes executables
  # app bundles, whose install rules these projects lack.
  mkCmake = { pname, cmakeFlags ? [ ], buildInputs ? [ ], env ? { }, ... }@args:
    let includes = lib.concatMapStringsSep " " (d: "-isystem ${lib.getDev d}/include") buildInputs;
    in ipkgs.xcodeClang.mkDerivation ({
      version = "0.1.0";
    } // removeAttrs args [ "cmakeFlags" "env" ] // {
      cmakeFlags = deps.iosCmakeFlags ++ [ "-DCMAKE_MACOSX_BUNDLE=OFF" ] ++ cmakeFlags;
      env = env // { CFLAGS = includes; CXXFLAGS = includes; };
    });

  processStats = mkCmake {
    pname = "process-stats-ios";
    src = inputs.process-stats;
    buildInputs = [ deps.nlohmann_json ];
    cmakeFlags = [ "-DPROCESS_STATS_BUILD_TESTS=OFF" ];
  };

  protocolLib = mkCmake {
    pname = "logos-protocol-ios-lib";
    src = inputs.logos-protocol;
    cmakeDir = "../cpp";
    buildInputs = [ deps.boost deps.openssl-shared deps.nlohmann_json ];
    cmakeFlags = [ "-DLOGOS_PROTOCOL_BUILD_QT=OFF" ];
  };
  # As the native logos-protocol-plain: the library, and the source headers a
  # module build reads under include/cpp.
  protocol = bpkgs.symlinkJoin {
    name = "logos-protocol-ios";
    paths = [ protocolLib inputs.logos-protocol.packages.aarch64-darwin.logos-protocol-include ];
  };

  # No process may be created: every container seam answers nullptr.
  containerNone = mkCmake {
    pname = "logos-container-none-ios";
    src = inputs.logos-container;
    buildInputs = [ deps.nlohmann_json ];
    cmakeFlags = [ "-DLOGOS_CONTAINER_BUILD_TESTS=OFF" ];
    installPhase = ''
      runHook preInstall
      cmake --install . --prefix $out --component none
      runHook postInstall
    '';
  };

  # The loader and the in-process host only: no host executable on iOS.
  loader = mkCmake {
    pname = "logos-module-loader-qt-ios";
    src = inputs.default-module-loader;
    buildInputs = [
      deps.boost deps.openssl-shared deps.spdlog deps.fmt deps.nlohmann_json protocol
      containerHeaders moduleLoaderHeaders
    ];
    cmakeFlags = [
      "-DLOGOS_BUILD_QT_HOST=OFF"
      "-DLOGOS_BUILD_PLAIN_HOST=OFF"
      "-DLOGOS_MODULE_LOADER_QT_BUILD_TESTS=OFF"
      "-DLOGOS_PROTOCOL_ROOT=${protocol}"
      "-DLOGOS_CONTAINER_ROOT=${containerHeaders}"
      "-DLOGOS_MODULE_LOADER_ROOT=${moduleLoaderHeaders}"
    ];
  };

  # liblgx.dylib alone: the lgx CLI has no use on a phone.
  lgx = mkCmake {
    pname = "liblgx-ios";
    src = lgxSrc;
    buildInputs = [ deps.nlohmann_json deps.libsodium semver ];
    cmakeFlags = [ "-DLGX_BUILD_SHARED=ON" "-DLGX_BUILD_TESTS=OFF" ];
    ninjaFlags = [ "lgx_shared" ];
    installPhase = ''
      runHook preInstall
      mkdir -p $out/lib $out/include
      cp liblgx.dylib $out/lib/
      cp $src/src/lgx.h $out/include/
      # logos/semver.hpp includes <semver/semver.hpp>: it ships beside it.
      cp -r $src/include/logos ${semver}/include/semver $out/include/
      runHook postInstall
    '';
  };

  packageManager = mkCmake {
    pname = "logos-package-manager-ios";
    src = inputs.logos-package-manager;
    buildInputs = [ deps.nlohmann_json ];
    cmakeFlags = [ "-DLGX_ROOT=${lgx}" ];
    ninjaFlags = [ "package_manager_lib" ];
    installPhase = ''
      runHook preInstall
      mkdir -p $out/lib $out/include
      cp lib/libpackage_manager_lib.dylib ${lgx}/lib/liblgx.dylib $out/lib/
      cp $src/src/package_manager_lib.h $src/src/lgpm.h ${lgx}/include/lgx.h $out/include/
      runHook postInstall
    '';
  };

  libpeering = mkCmake {
    pname = "libpeering-ios";
    src = inputs.logos-peering;
    nativeBuildInputs = [ bpkgs.pkg-config ];
    buildInputs = [ deps.openssl-shared deps.nlohmann_json deps.libblake3 deps.boost protocol ];
    propagatedBuildInputs = [ deps.libblake3 ];
    cmakeFlags = [ "-DLOGOS_PEERING_BUILD_TESTS=OFF" ];
  };

  liblogos = mkCmake {
    pname = "logos-liblogos-ios";
    src = liblogosFlake;
    nativeBuildInputs = [ bpkgs.pkg-config ];
    buildInputs = [
      deps.boost deps.openssl-shared deps.spdlog deps.fmt deps.nlohmann_json deps.libblake3
      protocol processStats containerHeaders containerNone moduleLoaderHeaders loader
      packageManager libpeering
    ];
    cmakeFlags = [
      "-DLOGOS_BUILD_TESTS=OFF"
      "-DLOGOS_PROTOCOL_ROOT=${protocol}"
      "-DPROCESS_STATS_ROOT=${processStats}"
      "-DLOGOS_CONTAINER_ROOT=${containerHeaders}"
      "-DLOGOS_MODULE_LOADER_ROOT=${moduleLoaderHeaders}"
      "-DLOGOS_PACKAGE_MANAGER_ROOT=${packageManager}"
    ];
    env.LOGOS_CAPABILITY_ENGINE_INCLUDE = "${inputs.logos-capability-module}/src";
  };

  # One module from its generated tree, with the builder's LogosModule.cmake and
  # the module flags its native build was given (transport, API style).
  mkModule = { name, tree, native, builder, ... }: mkCmake {
    pname = "${name}-ios";
    src = tree;
    nativeBuildInputs = [ bpkgs.pkg-config bpkgs.jq ];
    buildInputs = [ deps.boost deps.openssl-shared deps.nlohmann_json deps.libblake3 protocol ];
    cmakeFlags = [
      "-DLOGOS_CPP_SDK_ROOT=${builder.inputs.logos-cpp-sdk.packages.aarch64-darwin.default}"
      "-DLOGOS_PROTOCOL_ROOT=${protocol}"
    ] ++ lib.filter (f: lib.hasPrefix "-DLOGOS_MODULE_" f || lib.hasPrefix "-DLOGOS_API_STYLE" f)
      native.cmakeFlags;
    env = {
      LOGOS_MODULE_BUILDER_ROOT = "${builder}";
      LOGOS_PROTOCOL_ROOT = "${protocol}";
      # A peering module links libpeering; the tree stages the native one.
      LOGOS_EXT_ROOT_LOGOS_PEERING = "${libpeering}";
    };
    installPhase = ''
      runHook preInstall
      mkdir -p $out/lib
      for f in modules/${name}_plugin.dylib ${name}_plugin.dylib; do
        if [ -f "$f" ]; then cp "$f" $out/lib/; break; fi
      done
      image=$out/lib/${name}_plugin.dylib
      [ -f $image ] || { echo "no ${name}_plugin.dylib" >&2; exit 1; }
      sidecar=$out/lib/${name}_plugin.metadata.json
      cp ${tree}/metadata.json $sidecar
      chmod u+w $sidecar
      # In-process eligibility, read from this image as mkLogosModule's
      # inprocStamp reads the native one: only a plain module-ABI export map.
      nm -gU $image | awk '{print $NF}' | sed 's/^_//' > exports
      reason=""
      if [ "$(jq -r '.in_process // true' $sidecar)" = false ]; then
        reason="metadata sets in_process to false"
      elif ! grep -qx logos_module_set_runtime_delegate exports; then
        reason="no logos_module_set_runtime_delegate export"
      elif grep -qv '^logos_module_' exports; then
        reason="exports symbols other than logos_module_*"
      elif nm -gUm $image | grep -q 'weak external'; then
        reason="exports weak definitions"
      fi
      jq --arg reason "$reason" \
        'if $reason == "" then . + {inproc_eligible: true}
         else . + {inproc_eligible: false, inproc_ineligible_reason: $reason} end' \
        $sidecar > sidecar.json
      mv sidecar.json $sidecar
      echo "in-process eligible: $(jq -c '[.inproc_eligible, .inproc_ineligible_reason]' $sidecar)"
      runHook postInstall
    '';
  };
  modules = map (m: m // { built = mkModule m; }) moduleTrees;

  # What the app links: liblogos and the plain protocol image it shares.
  hostLibs = bpkgs.runCommand "logos-ios-host-libs" { } ''
    mkdir -p $out/lib
    ln -s ${liblogos}/lib/liblogos_core.dylib ${protocol}/lib/liblogos_protocol_plain.dylib $out/lib/
  '';

  toolchain = bpkgs.rust-bin.stable."1.96.0".default.override { targets = [ triple ]; };
  rustPlatform = bpkgs.makeRustPlatform { cargo = toolchain; rustc = toolchain; };
  # Slint draws with Skia over Metal on iOS; skia-bindings would download this.
  skiaBinaries = bpkgs.fetchurl {
    url = "https://github.com/rust-skia/skia-binaries/releases/download/0.153.3/skia-binaries-b7f043e0b1e2a850e702-aarch64-apple-ios-sim-ganesh-gl-jpegd-jpege-metal-pdf.tar.gz";
    hash = "sha256-Ox1wpnPwGjCKeqiDtKL6pockZiVhUxGx2KM5Hy8bxNo=";
  };

  # DEVELOPER_DIR is Xcode's, so xcrun finds the simulator SDK. nix's clang
  # takes the macOS SDK from it too, and its ld cannot read Xcode's: the build
  # scripts' tools get nix's SDK back.
  hostTool = tool: bpkgs.writeShellScript "host-${tool}" ''
    export DEVELOPER_DIR="''${SDKROOT%/Platforms/*}"
    exec ${bpkgs.stdenv.cc}/bin/${tool} "$@"
  '';

  program = rustPlatform.buildRustPackage {
    pname = "logos-core-demo-ios";
    version = "0.1.0";
    inherit src;
    cargoLock = { lockFile = src + "/Cargo.lock"; allowBuiltinFetchGit = true; };
    __noChroot = true;
    nativeBuildInputs = [ xcodeWrapper ];
    env = {
      LOGOS_HOST_LIB_DIR = "${hostLibs}/lib";
      SKIA_BINARIES_URL = "file://${skiaBinaries}";
      IPHONEOS_DEPLOYMENT_TARGET = minIos;
      CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER = hostTool "cc";
      CC_aarch64_apple_darwin = hostTool "cc";
      CXX_aarch64_apple_darwin = hostTool "c++";
    };
    # The stdenv here is macOS's; Xcode's clang builds and links for the target.
    buildPhase = ''
      runHook preBuild
      export CARGO_HOME=$TMPDIR/cargo
      export DEVELOPER_DIR=${xcodeWrapper.developerDir}
      target_env() { export "$1_aarch64_apple_ios_sim=$(xcrun --sdk ${sdk} --find $2)"; }
      target_env CC clang; target_env CXX clang++; target_env AR ar
      export CARGO_TARGET_AARCH64_APPLE_IOS_SIM_LINKER=$CC_aarch64_apple_ios_sim
      export CARGO_TARGET_AARCH64_APPLE_IOS_SIM_RUSTFLAGS="-C link-arg=-Wl,-rpath,@executable_path/Frameworks"
      # SDKROOT stays macOS's, for the build scripts: rustc and cc-rs ignore an
      # SDKROOT of another platform and ask xcrun for the simulator's.
      cargo build --release --offline --target ${triple} \
        -p logos-core-demo -p logos-core-demo-headless \
        --bin logos-core-demo --bin logos-core-demo-headless
      runHook postBuild
    '';
    installPhase = ''
      runHook preInstall
      mkdir -p $out/bin
      cp target/${triple}/release/logos-core-demo target/${triple}/release/logos-core-demo-headless $out/bin/
      runHook postInstall
    '';
    doCheck = false;
    dontStrip = true;
    dontFixup = true;
  };

  infoPlist = ./ios/Info.plist;

  # The .app: the program (and the headless client, for `simctl spawn`),
  # Frameworks/ with every library they and the modules load, the modules as the
  # desktop bundles lay them out (modules/, app-modules/), all ad-hoc signed.
  app = bpkgs.runCommand "logos-core-demo-ios-app" {
    __noChroot = true;
    nativeBuildInputs = [ xcodeWrapper bpkgs.python3 bpkgs.jq ];
  } (''
    app=$out/LogosCoreDemo.app
    mkdir -p $app/Frameworks $app/modules $app/app-modules
    cp ${program}/bin/logos-core-demo ${program}/bin/logos-core-demo-headless $app/
    cp ${infoPlist} $app/Info.plist
    cp -L ${liblogos}/lib/liblogos_core.dylib ${protocol}/lib/liblogos_protocol_plain.dylib \
      ${packageManager}/lib/libpackage_manager_lib.dylib ${packageManager}/lib/liblgx.dylib \
      ${deps.openssl-shared}/lib/libssl.3.dylib ${deps.openssl-shared}/lib/libcrypto.3.dylib \
      $app/Frameworks/
  '' + lib.concatMapStrings (m: ''
    dir=$app/${if m.group == "app" then "app-modules" else "modules"}/${m.name}
    mkdir -p $dir
    cp -L ${m.built}/lib/* $dir/
    version=$(jq -r .version $dir/${m.name}_plugin.metadata.json)
    jq -n --arg n ${m.name} --arg v "$version" --arg p ${m.name}_plugin.dylib \
      '{name: $n, version: $v, type: "core", main: {"ios-sim-arm64-dev": $p, "ios-sim-arm64": $p}}' \
      > $dir/manifest.json
  '') modules + ''
    chmod -R u+w $out
    python3 ${./ios-bundle.py} $app
  '');
in
{
  inherit ipkgs deps processStats protocol containerNone loader lgx packageManager libpeering liblogos
    hostLibs program app;
  modules = lib.listToAttrs (map (m: lib.nameValuePair m.name m.built) modules);
}
