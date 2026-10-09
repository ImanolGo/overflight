# AUR packaging

This directory holds the `PKGBUILD` for the Arch User Repository. AUR packages
live in their own git repository, so this file is kept here for reference and
then pushed to AUR as its own repo.

## Test it locally

```sh
cd packaging/aur
makepkg --printsrcinfo > .SRCINFO   # regenerate metadata
makepkg -f                          # build overflight-<ver>-*.pkg.tar.zst
pacman -Qip overflight-*.pkg.tar.zst
```

## Publish to the AUR

Requires an AUR account with your SSH key registered
(see https://wiki.archlinux.org/title/AUR_submission_guidelines).

```sh
git clone ssh://aur@aur.archlinux.org/overflight.git aur-overflight
cp PKGBUILD .SRCINFO aur-overflight/
cd aur-overflight
git add PKGBUILD .SRCINFO
git commit -m "overflight 0.1.0"
git push
```

## Updating for a new release

1. Bump `pkgver`, reset `pkgrel=1`.
2. Point `source` at the new tag and refresh `sha256sums`
   (`updpkgsums` does this for you).
3. `makepkg --printsrcinfo > .SRCINFO` and push.

The `check()` function runs the normal test suite, which is headless and does
not need a terminal.
