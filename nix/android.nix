# The Android app: the Slint UI as a NativeActivity library, and an APK that
# carries it with the runtime's executables, plugins and libraries.
{ lib, logos-nix, nixpkgs, rust-overlay, logos-rust-sdk, src }:

let
  target = logos-nix.lib.mobileTargets.aarch64-android;
  apkgs = target.pkgs;
  # Toolchains run on the build machine and target Android.
  bpkgs = import nixpkgs {
    system = target.buildSystem;
    overlays = logos-nix.lib.nativeOverlays ++ [ (import rust-overlay) ];
  };
  triple = "aarch64-linux-android";
  toolchain = bpkgs.rust-bin.stable."1.96.0".default.override { targets = [ triple ]; };
  rustPlatform = bpkgs.makeRustPlatform { cargo = toolchain; rustc = toolchain; };

  # Slint renders with Skia on Android; skia-bindings would download this.
  skiaBinaries = bpkgs.fetchurl {
    url = "https://github.com/rust-skia/skia-binaries/releases/download/0.153.3/skia-binaries-b7f043e0b1e2a850e702-aarch64-linux-android-ganesh-gl-jpegd-jpege-pdf-vulkan.tar.gz";
    hash = "sha256-DZF7YI0DYy+oQo4A/3FbvlZj+JYYiTpeAmX/6j5s9Rk=";
  };
in
{
  inherit apkgs;

  # An SDK with the emulator and an arm64 API 34 image, for this Mac.
  emulatorSdk = system: ((logos-nix.lib.androidBuildPkgs system).androidenv.composeAndroidPackages {
    platformVersions = [ "34" ];
    includeEmulator = true;
    includeSystemImages = true;
    systemImageTypes = [ "default" ];
    abiVersions = [ "arm64-v8a" ];
    includeNDK = false;
  }).androidsdk;

  # liblogos_core_demo.so, linked against liblogos' Android lib output.
  appLib = { liblogosLib }:
    let host = logos-rust-sdk.lib.hostBuildSupportAndroid { inherit liblogosLib; androidPkgs = apkgs; };
    in rustPlatform.buildRustPackage {
    pname = "logos-core-demo-android";
    version = "0.1.0";
    inherit src;
    cargoLock = { lockFile = src + "/Cargo.lock"; allowBuiltinFetchGit = true; };
    nativeBuildInputs = host.nativeBuildInputs ++ [ bpkgs.jdk17 ];
    env = host.env // {
      # Slint's Android backend compiles a Java helper against android.jar.
      ANDROID_HOME = apkgs.androidPkgs.sdkRoot;
      JAVA_HOME = bpkgs.jdk17.home;
      SKIA_BINARIES_URL = "file://${skiaBinaries}";
    };
    # The stdenv here is the build machine's; drive cargo for the target.
    buildPhase = ''
      runHook preBuild
      export CARGO_HOME=$TMPDIR/cargo
      cargo build --release --offline --target ${triple} -p logos-core-demo --lib
      runHook postBuild
    '';
    installPhase = ''
      runHook preInstall
      mkdir -p $out/lib
      cp target/${triple}/release/liblogos_core_demo.so $out/lib/
      runHook postInstall
    '';
    doCheck = false;
    dontStrip = true;
  };

  # The APK. `modules` are { group; name; version; dir; } with dir holding
  # <name>_plugin.so and its sidecar; groups become <files>/modules/<group>/.
  apk = { appLib, executables, libraries, modules, searchPath }:
    let
      assets = apkgs.runCommand "logos-core-demo-assets" { nativeBuildInputs = [ apkgs.buildPackages.jq ]; } (''
        mkdir -p $out/modules
        echo '{}' > index.json
      '' + lib.concatMapStrings (m: ''
        dir=$out/modules/${m.group}/${m.name}
        mkdir -p $dir
        plugin=${m.name}_plugin.so
        [ -f ${m.dir}/$plugin ] || { echo "no $plugin in ${m.dir}" >&2; exit 1; }
        for f in ${m.dir}/*.metadata.json; do if [ -e "$f" ]; then cp "$f" $dir/; fi; done
        cat > $dir/manifest.json <<EOF
        {
          "name": "${m.name}",
          "version": "${m.version}",
          "type": "core",
          "main": {
            "linux-aarch64-dev": "$plugin",
            "linux-arm64-dev": "$plugin",
            "linux-aarch64": "$plugin",
            "linux-arm64": "$plugin"
          }
        }
        EOF
        files=$(cd $dir && ls | jq -R . | jq -s .)
        jq --arg g ${m.group} --arg n ${m.name} --arg p "$plugin" --argjson f "$files" \
          '.[$g][$n] = {files: $f, plugin: $p}' index.json > index.new && mv index.new index.json
      '') modules + ''
        cp index.json $out/modules/index.json
      '');
    in
    apkgs.mkNativeActivityApk {
      pname = "logos-core-demo";
      version = "0.1.0";
      packageName = "co.logos.coredemo";
      libName = "logos_core_demo";
      label = "Logos Core Demo";
      libDirs = [ "${appLib}/lib" ];
      inherit executables libraries searchPath assets;
      librariesAs = lib.listToAttrs (map (m: {
        name = "lib${m.name}_plugin.so";
        value = "${m.dir}/${m.name}_plugin.so";
      }) modules);
      permissions = [ "android.permission.INTERNET" "android.permission.ACCESS_NETWORK_STATE" ];
      # An invite opens the app (a QR code scanned by the camera, a link), and
      # LogosActivity keeps one that arrives while it runs.
      launchMode = "singleTask";
      activity = "co.logos.coredemo.LogosActivity";
      javaSources = [ ../app/android/java/co/logos/coredemo/LogosActivity.java ];
      intentFilters = ''
        <intent-filter>
          <action android:name="android.intent.action.VIEW"/>
          <category android:name="android.intent.category.DEFAULT"/>
          <category android:name="android.intent.category.BROWSABLE"/>
          <data android:scheme="logos-pair"/>
        </intent-filter>
      '';
    };
}
