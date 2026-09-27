# Third-party C/C++ dependencies of the Logos runtime for iOS: static archives,
# plus one shared OpenSSL, built with Xcode's clang (logos-nix's xcodeClang).
# Their sources are the pin iosPkgs comes from (its buildPackages).
#   deps = import ./ios-deps.nix { iosPkgs = logos-nix.lib.mkIosPkgs { ... }; };
{
  iosPkgs,
  # Deployment target of every object (LC_BUILD_VERSION minos).
  minIos ? "17.0",
}:

let
  inherit (iosPkgs) lib xcodeClang;
  bp = iosPkgs.buildPackages;
  hp = iosPkgs.stdenv.hostPlatform;

  simulator = hp.darwinPlatform == "ios-simulator";
  sdk = if simulator then "iphonesimulator" else "iphoneos";
  arch = hp.darwinArch;
  # LC_BUILD_VERSION platform: 7 = IOSSIMULATOR, 2 = IOS.
  platformId = if simulator then "7" else "2";
  minFlag = "${if simulator then "-mios-simulator-version-min" else "-miphoneos-version-min"}=${minIos}";

  # Also what a CMake consumer of these archives passes. CMake's iOS platform
  # re-roots every find_* into the SDK (mode ONLY), which hides /nix/store.
  iosCmakeFlags = [
    "-DCMAKE_OSX_SYSROOT=${sdk}"
    "-DCMAKE_OSX_ARCHITECTURES=${arch}"
    "-DCMAKE_OSX_DEPLOYMENT_TARGET=${minIos}"
    "-DCMAKE_FIND_ROOT_PATH_MODE_PACKAGE=BOTH"
    "-DCMAKE_FIND_ROOT_PATH_MODE_LIBRARY=BOTH"
    "-DCMAKE_FIND_ROOT_PATH_MODE_INCLUDE=BOTH"
  ];

  staticCmakeFlags = iosCmakeFlags ++ [
    "-DBUILD_SHARED_LIBS=OFF"
    "-DCMAKE_POSITION_INDEPENDENT_CODE=ON"
    "-DCMAKE_CXX_STANDARD=17"
  ];

  # Non-CMake builds: clang takes the sysroot from SDKROOT (as under xcrun).
  exportSdkroot = ''
    export SDKROOT="$(xcrun --sdk ${sdk} --show-sdk-path)"
  '';

  # Static archives only, thin `arch`, and every member object built for this
  # platform and deployment target.
  checkArchives = ''
    _dynamic=$(find "$out" \( -name '*.dylib' -o -name '*.so' -o -name '*.framework' \) -print)
    if [ -n "$_dynamic" ]; then
      echo "error: dynamic image in a static-only iOS dependency:"$'\n'"$_dynamic" >&2
      exit 1
    fi
    _archives=0
    while IFS= read -r -d "" _a; do
      _archives=$((_archives + 1))
      lipo -info "$_a" | grep -q "is architecture: ${arch}\$" \
        || { echo "error: $_a is not thin ${arch}: $(lipo -info "$_a")" >&2; exit 1; }
      otool -l "$_a" | awk -v p=${platformId} -v m=${minIos} '
        function done_member() { if (member != "" && !seen) { print "error: " member " has no LC_BUILD_VERSION"; bad = 1 } }
        /^[^ ].*\(.*\):$/ { done_member(); member = $0; seen = 0; next }
        $1 == "cmd" { inbv = ($2 == "LC_BUILD_VERSION"); next }
        inbv && $1 == "platform" && $2 != p { print "error: " member " platform " $2; bad = 1 }
        inbv && $1 == "minos" { seen = 1; if ($2 != m) { print "error: " member " minos " $2; bad = 1 } }
        END { done_member(); exit bad }' >&2 || exit 1
    done < <(find "$out" -name '*.a' -print0)
    [ "$_archives" -gt 0 ] || { echo "error: no static archive installed" >&2; exit 1; }
    echo "checked $_archives archive(s): thin ${arch}, every member platform ${platformId} minos ${minIos}"
  '';

  # Dylibs: thin `arch`, this platform and deployment target, an @rpath
  # install name, a valid signature, and no /nix/store in any load command.
  checkDylibs = ''
    [ -z "$(find "$out" -name '*.a' -print -quit)" ] || { echo "error: static archive in a shared-only output" >&2; exit 1; }
    _dylibs=0
    for _d in $(find "$out" -name '*.dylib' -type f); do
      _dylibs=$((_dylibs + 1))
      lipo -info "$_d" | grep -q "is architecture: ${arch}\$" \
        || { echo "error: $_d is not thin ${arch}: $(lipo -info "$_d")" >&2; exit 1; }
      _bv=$(otool -l "$_d" | grep -A4 LC_BUILD_VERSION)
      grep -q 'platform ${platformId}$' <<<"$_bv" && grep -q 'minos ${minIos}$' <<<"$_bv" \
        || { echo "error: $_d: $_bv" >&2; exit 1; }
      [ "$(otool -D "$_d" | tail -n 1)" = "@rpath/$(basename "$_d")" ] \
        || { echo "error: $_d install name is $(otool -D "$_d" | tail -n 1)" >&2; exit 1; }
      # (line 1 of otool's output is the file's own path)
      if otool -l "$_d" | sed 1d | grep -q /nix/store; then
        echo "error: $_d has a /nix/store path in a load command:" >&2
        otool -l "$_d" | sed 1d | grep /nix/store >&2
        exit 1
      fi
      codesign --verify "$_d"
    done
    [ "$_dylibs" -gt 0 ] || { echo "error: no dylib installed" >&2; exit 1; }
    echo "checked $_dylibs dylib(s): thin ${arch}, platform ${platformId} minos ${minIos}, @rpath ids, signed"
  '';

  mkStatic =
    attrs:
    xcodeClang.mkDerivation (
      attrs
      // {
        postInstall = (attrs.postInstall or "") + checkArchives;
        passthru = (attrs.passthru or { }) // {
          inherit iosCmakeFlags sdk minIos;
        };
      }
    );

  opensslTarget = if simulator then "iossimulator-arm64-xcrun" else "ios64-xcrun";

  # CC is Xcode's clang (xcodeClang); -arch comes from the target. no-module
  # puts the legacy provider in libcrypto rather than a loadable module.
  opensslConfigurePhase = linkage: ''
    runHook preConfigure
    ${exportSdkroot}
    perl ./Configure ${opensslTarget} ${linkage} \
      no-module no-tests no-apps no-docs \
      --prefix="$out" --libdir=lib --openssldir=/etc/ssl \
      ${minFlag}
    runHook postConfigure
  '';

  # Plain make instead of the cmake/ninja phases xcodeClang defaults to.
  noCmake = {
    dontUseCmakeConfigure = true;
    dontUseNinjaBuild = true;
    dontUseNinjaInstall = true;
    dontUseNinjaCheck = true;
  };

  # Libraries the runtime builds; the rest of Boost ships as headers only.
  # Not process: its compiled part fails on iOS, only in src/shell.cpp, whose
  # wordexp()/wordfree() the iOS SDK marks unavailable.
  boostLibraries = [
    "filesystem"
    "atomic"
    "system"
    "asio"
    "dll"
    "uuid"
  ];
in
rec {
  # What a CMake consumer of these passes.
  inherit iosCmakeFlags;

  boost = mkStatic {
    pname = "boost";
    inherit (bp.boost) version src;

    # The release tarball (nixpkgs' source) is b2-shaped: no root
    # CMakeLists.txt, and no libs/*/include, since every library's headers are
    # merged into the top-level boost/. So: the superproject's root file, an
    # empty include/ per library for its targets to name, and the merged tree
    # on every compile. Headers are then installed from boost/ below.
    postPatch = ''
      for _l in libs/*/ libs/numeric/*/; do
        if [ -f "$_l/CMakeLists.txt" ]; then mkdir -p "$_l/include"; fi
      done
      cat > CMakeLists.txt <<'EOF'
      cmake_minimum_required(VERSION 3.8...3.20)
      project(Boost VERSION ${bp.boost.version} LANGUAGES CXX)
      set(BOOST_SUPERPROJECT_VERSION ''${PROJECT_VERSION})
      set(BOOST_SUPERPROJECT_SOURCE_DIR ''${PROJECT_SOURCE_DIR})
      include_directories(''${PROJECT_SOURCE_DIR})
      list(APPEND CMAKE_MODULE_PATH ''${PROJECT_SOURCE_DIR}/tools/cmake/include)
      include(BoostRoot)
      EOF
    '';

    cmakeFlags = staticCmakeFlags ++ [
      "-DBOOST_INCLUDE_LIBRARIES=${lib.concatStringsSep ";" boostLibraries}"
      "-DBOOST_INSTALL_LAYOUT=system"
    ];

    # All headers, as every other Boost package ships them.
    postInstall = ''
      cp -R ../boost "$out/include/"
    '';
  };

  openssl = mkStatic (
    noCmake
    // {
      pname = "openssl";
      inherit (bp.openssl) version src;
      nativeBuildInputs = [ bp.perl ];
      configurePhase = opensslConfigurePhase "no-shared";
      installTargets = "install_sw";
    }
  );

  # One OpenSSL image per process: liblogos_protocol_plain, liblogos_core and
  # every module dylib must share its global state, not carry a copy each.
  # A separate output so FindOpenSSL cannot fall back to an archive.
  openssl-shared = xcodeClang.mkDerivation (
    noCmake
    // {
      pname = "openssl-shared";
      inherit (bp.openssl) version src;
      nativeBuildInputs = [ bp.perl ];
      configurePhase = opensslConfigurePhase "shared";
      installTargets = "install_sw";

      # OpenSSL links the dylibs with absolute $(libdir) install names.
      postInstall = ''
        rm "$out/lib/libssl.a" "$out/lib/libcrypto.a"
        _crypto=$(otool -L "$out/lib/libssl.3.dylib" | awk '/libcrypto/ { print $1 }')
        install_name_tool -id @rpath/libcrypto.3.dylib "$out/lib/libcrypto.3.dylib"
        install_name_tool -id @rpath/libssl.3.dylib \
          -change "$_crypto" @rpath/libcrypto.3.dylib "$out/lib/libssl.3.dylib"
        ${checkDylibs}
      '';

      passthru = {
        inherit iosCmakeFlags sdk minIos;
      };
    }
  );

  fmt = mkStatic {
    pname = "fmt";
    inherit (bp.fmt) version src;
    cmakeFlags = staticCmakeFlags ++ [
      "-DFMT_TEST=OFF"
      "-DFMT_DOC=OFF"
      "-DFMT_INSTALL=ON"
    ];
  };

  spdlog = mkStatic {
    pname = "spdlog";
    inherit (bp.spdlog) version src;
    # spdlogConfig.cmake calls find_dependency(fmt).
    propagatedBuildInputs = [ fmt ];
    cmakeFlags = staticCmakeFlags ++ [
      "-DSPDLOG_FMT_EXTERNAL=ON"
      "-DSPDLOG_BUILD_SHARED=OFF"
      "-DSPDLOG_BUILD_PIC=ON"
      "-DSPDLOG_BUILD_EXAMPLE=OFF"
      "-DSPDLOG_BUILD_TESTS=OFF"
      "-DSPDLOG_BUILD_BENCH=OFF"
      "-DSPDLOG_INSTALL=ON"
    ];
    # As nixpkgs does, so a consumer without the CMake target still uses external fmt.
    postInstall = ''
      substituteInPlace "$out/include/spdlog/tweakme.h" \
        --replace-fail '// #define SPDLOG_FMT_EXTERNAL' '#define SPDLOG_FMT_EXTERNAL'
    '';
  };

  libsodium = mkStatic (
    noCmake
    // {
      pname = "libsodium";
      inherit (bp.libsodium) version src;
      # Upstream's dist-build/apple-xcframework.sh recipe for the arm64 simulator;
      # the flags ride in CC so autoconf's preprocessor checks see them too.
      configurePhase = ''
        runHook preConfigure
        ${exportSdkroot}
        export CC="$CC -arch ${arch} ${minFlag}" CFLAGS="-O3"
        ./configure --prefix="$out" \
          --build=aarch64-apple-darwin --host=aarch64-apple-darwin23 \
          --disable-shared --enable-static --with-pic --disable-dependency-tracking
        runHook postConfigure
      '';
    }
  );

  libblake3 = mkStatic {
    pname = "libblake3";
    inherit (bp.libblake3) version src;
    sourceRoot = "${bp.libblake3.src.name}/c";
    cmakeFlags = staticCmakeFlags ++ [
      "-DBLAKE3_USE_TBB=OFF"
      "-DBLAKE3_FETCH_TBB=OFF"
    ];
  };

  # Header-only and arch-independent (its version file has no pointer-size
  # check), so nixpkgs' build-platform package serves an iOS consumer as is.
  nlohmann_json = bp.nlohmann_json;
}
