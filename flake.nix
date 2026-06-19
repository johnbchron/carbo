{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    devshell.url = "github:numtide/devshell";
  };

  outputs = { nixpkgs, rust-overlay, devshell, flake-utils, ... }: 
    flake-utils.lib.eachDefaultSystem (system: let
      pkgs = import nixpkgs {
        inherit system;
        overlays = [
          (import rust-overlay)
          devshell.overlays.default
        ];
      };

      toolchain_fn = p: p.rust-bin.selectLatestNightlyWith (toolchain: toolchain.default.override {
        extensions = [ "rust-src" "rust-analyzer" ];
      });

      common-packages = with pkgs; [
        pkg-config clang mold

        fontconfig
        ( toolchain_fn pkgs )
      ];

      linux-packages = with pkgs; [
        pkg-config
        
        alsa-lib udev

        fontconfig freetype

        libxkbcommon wayland
        # xorg.libX11 xorg.libXcursor xorg.libXi xorg.libXrandr

        vulkan-headers vulkan-loader
        vulkan-tools vulkan-tools-lunarg
        vulkan-extension-layer
        # vulkan-validation-layers
      ];

      make-pkg-config-path = packages:
        pkgs.lib.concatStringsSep ":" (
          pkgs.lib.concatMap
            (pkg: map (sub: "${pkgs.lib.getDev pkg}/${sub}") [ "lib/pkgconfig" "share/pkgconfig" ])
            packages
        );

      linux-devshell = pkgs.devshell.mkShell (let
        packages = common-packages ++ linux-packages;
      in {
        inherit packages;
        motd = "\n  Welcome to the {2}$(basename $PRJ_ROOT){reset} shell.\n";
        env = [
          { name = "LD_LIBRARY_PATH"; value = pkgs.lib.makeLibraryPath packages; }
          { name = "PKG_CONFIG_PATH"; value = make-pkg-config-path packages; }
        ];

        commands = [
          {
            name = "run";
            command = "cargo run -p carbo $@";
            help = "builds and runs the terminal";
          }
          {
            name = "runp";
            command = "cargo run -p carbo --release --features profile $@";
            help = "builds and runs the terminal in release mode with profiling";
          }
        ];
      });
      darwin-devshell = pkgs.mkShell (let
        packages = common-packages ++ [ pkgs.apple-sdk ];
      in {
        nativeBuildInputs = packages;
        LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath packages;
      });
    in {
      devShell = if pkgs.stdenv.isLinux then linux-devshell else darwin-devshell;
  });
}
