{
  lib,
  rustPlatform,
  pkg-config,
  makeWrapper,
  dbus,
  fontconfig,
  wayland,
  libxkbcommon,
  vulkan-loader,
  noto-fonts-cjk-sans-static,
}:

rustPlatform.buildRustPackage {
  pname = "arcglyph";
  version = (lib.importTOML ../Cargo.toml).package.version;

  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../src
      ../packaging/arcglyph.desktop
      ../packaging/60-arcglyph-uinput.rules
      ../packaging/gnome-extension
    ];
  };

  cargoHash = "sha256-J4XQqCqcavKGs1OfXh17YeU+xBsBNtCwxjIhwkUGcfk=";

  nativeBuildInputs = [
    makeWrapper
    pkg-config
  ];

  buildInputs = [
    dbus
    fontconfig
    wayland
    libxkbcommon
    vulkan-loader
  ];

  postInstall = ''
    install -Dm644 packaging/arcglyph.desktop \
      "$out/share/applications/arcglyph.desktop"
    substituteInPlace "$out/share/applications/arcglyph.desktop" \
      --replace-fail "Exec=/usr/bin/arcglyph" "Exec=$out/bin/arcglyph"
    install -Dm644 packaging/60-arcglyph-uinput.rules \
      "$out/lib/udev/rules.d/60-arcglyph-uinput.rules"
  '';

  postFixup = ''
    wrapProgram "$out/bin/arcglyph" \
      --prefix LD_LIBRARY_PATH : "${
        lib.makeLibraryPath [
          wayland
          libxkbcommon
          vulkan-loader
        ]
      }" \
      --set-default ARCGLYPH_AUTOSTART_EXEC "$out/bin/arcglyph" \
      --set ARCGLYPH_FONT "${noto-fonts-cjk-sans-static}/share/fonts/opentype/noto-cjk/NotoSansCJK-Regular.ttc"
  '';

  meta = {
    description = "Right-click mouse-gesture daemon for Wayland";
    homepage = "https://github.com/lu5je0/arcglyph";
    license = lib.licenses.mit;
    mainProgram = "arcglyph";
    platforms = lib.platforms.linux;
  };
}
