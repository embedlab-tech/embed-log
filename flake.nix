{
  description = "embed-log";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
        };

        python = pkgs.python3;
        project = (builtins.fromTOML (builtins.readFile ./pyproject.toml)).project;
        pythonBuildInputs = ps: with ps; [
          hatchling
        ];

        pythonRuntimeInputs = ps: with ps; [
          aiohttp
          cbor2
          pyserial
          pyyaml
          watchdog

          # Optional network-capture extra from pyproject.toml.
          scapy
        ];

        embedLog = python.pkgs.buildPythonApplication {
          pname = project.name;
          inherit (project) version;
          src = self;

          pyproject = true;

          nativeBuildInputs = pythonBuildInputs python.pkgs;

          propagatedBuildInputs = pythonRuntimeInputs python.pkgs;

          pythonImportsCheck = [
            "backend.server"
          ];

          meta = {
            inherit (project) description;
            homepage = "https://github.com/embedlab-tech/embed-log";
            license = pkgs.lib.licenses.mit;
            mainProgram = "embed-log";
          };
        };
      in
      {
        packages.default = embedLog;
        packages.embed-log = embedLog;

        apps.default = {
          type = "app";
          program = "${embedLog}/bin/embed-log";
          meta.description = project.description;
        };

        devShells.default = pkgs.mkShell {
          packages = [
            (python.withPackages (ps: (pythonBuildInputs ps) ++ (pythonRuntimeInputs ps)))
            pkgs.uv
            pkgs.nodejs
          ];
        };
      }
    );
}
