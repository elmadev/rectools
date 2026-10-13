#!/bin/sh
# Builds @rectools/react-rec-viewer and copies it over the one installed in a site, to try
# the player there before it is published:
#
#   scripts/try-in-site.sh ~/projects/elmaonline-site/web
#
# The site's package.json and yarn.lock are left alone. `yarn install --check-files` in the
# site puts the published version back.
set -eu

site=$(cd "${1:?usage: scripts/try-in-site.sh <site folder with package.json>}" && pwd)
root=$(cd "$(dirname "$0")/.." && pwd)
pkg="$root/packages/react-rec-viewer"
dest="$site/node_modules/@rectools/react-rec-viewer"

(cd "$root" && pnpm build)

rm -rf "$dest"
mkdir -p "$dest"
cp -R "$pkg/dist" "$pkg/wasm" "$pkg/README.md" "$pkg/LICENSE.md" "$pkg/CHANGELOG.md" "$dest/"
# The published exports, which point at dist/ rather than the sources
node -e '
const fs = require("fs");
const [src, dest] = process.argv.slice(1);
const pkg = JSON.parse(fs.readFileSync(src, "utf8"));
pkg.exports = pkg.publishConfig.exports;
delete pkg.publishConfig;
fs.writeFileSync(dest, JSON.stringify(pkg, null, 2) + "\n");
' "$pkg/package.json" "$dest/package.json"

# Vite's dev server keeps serving the files it had
rm -rf "$site/node_modules/.vite"
echo "Copied the player into $dest"
echo "Restart the site's dev server and reload the page with the cache emptied."
