# nixos-limine

A [limine](https://limine-bootloader.org/) installer for NixOS, written in
rust, and a `boot.loader.limine` module that uses it in place of the one in
nixpkgs.

## Why

The installer nixpkgs ships is a python script that treats its input as an
untyped dictionary and ignores the exit status of every tool it runs, so a
failed `limine bios-install` or `sbctl sign` looks exactly like a successful
one. This one validates its configuration into types where the contradictory
states are unreachable before it touches the boot filesystem, fails when the
tools it calls fail, and reports what they printed when they do.

It also plans the whole install before performing any of it: rendering
limine.conf only reads, so a run that is going to fail does so before anything
on the ESP has changed.

## Use

```nix
{
  inputs.nixos-limine.url = "github:parzivale/nixos-limine";

  outputs = { nixpkgs, nixos-limine, ... }: {
    nixosConfigurations.machine = nixpkgs.lib.nixosSystem {
      modules = [
        nixos-limine.nixosModules.default
        {
          boot.loader.limine.enable = true;
          boot.loader.limine.settings.timeout = 5;
        }
      ];
    };
  };
}
```

The module replaces nixpkgs' `boot.loader.limine` at the same option path, so
`disabledModules` is handled for you.

## Tests

`nix flake check` builds the package, runs its unit tests, and boots two VMs
through limine: one installed to the removable media path, one registered in
NVRAM with the fallback path absent, so only the entry the installer wrote can
start it.

## Status

The BIOS half is unexercised: limine's stage 1 is x86 only, and the tests here
have so far only been run on aarch64.
