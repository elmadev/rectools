#!/bin/sh
# Builds @rectools/react-rec-viewer, packs it the way a publish would and installs that into
# a site using yarn 1, to try the player before it is published:
#
#   scripts/try-in-site.sh ~/projects/elmaonline-site/web
#
# The site's package.json and yarn.lock then point at the local tarball, so put them back
# before committing.
set -eu

site=$(cd "${1:?usage: scripts/try-in-site.sh <site folder with package.json>}" && pwd)
root=$(cd "$(dirname "$0")/.." && pwd)
out="$HOME/rectools-pack"
name=@rectools/react-rec-viewer

mkdir -p "$out"
rm -f "$out"/rectools-react-rec-viewer-*.tgz
(cd "$root" && pnpm build)
(cd "$root/packages/react-rec-viewer" && pnpm pack --pack-destination "$out")
tgz=$(ls "$out"/rectools-react-rec-viewer-*.tgz)

cd "$site"
# yarn 1 keeps a local tarball by its path, in its cache and in its temp folder, and would
# install the old one again
cache=$(yarn cache dir)
rm -rf "$cache"/npm-@rectools-react-rec-viewer-* "node_modules/$name" node_modules/.yarn-integrity
for dir in "$cache"/.tmp/*; do
    if grep -qs "\"name\": \"$name\"" "$dir/package.json"; then rm -rf "$dir"; fi
done

TGZ="$tgz" NAME="$name" node -e '
const fs = require("fs");
const pkg = JSON.parse(fs.readFileSync("package.json", "utf8"));
pkg.dependencies[process.env.NAME] = `file:${process.env.TGZ}`;
fs.writeFileSync("package.json", JSON.stringify(pkg, null, 2) + "\n");
if (fs.existsSync("yarn.lock")) {
  const blocks = fs.readFileSync("yarn.lock", "utf8").split("\n\n");
  const kept = blocks.filter((b) => !b.trimStart().startsWith(`"${process.env.NAME}@`));
  fs.writeFileSync("yarn.lock", kept.join("\n\n"));
}
'
yarn install
# Vite's dev server keeps serving the files it had, so it has to start again
rm -rf node_modules/.vite
echo "Installed $tgz into $site"
echo "Restart the site's dev server (yarn start) and reload the page with the cache emptied."
