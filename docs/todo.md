# To do

- A release flow with a changelog (release-plz or changesets). Everything is 0.1.0, with
  separate tags for the builds (`v*`) and npm (`react-rec-viewer-v*`).
- Signed and notarized macOS builds from CI. They need an Apple developer account.
- JS lint and component tests.
- `clap` for the command line parsers, and distinct exit codes for usage and load errors.
- The `directories` crate for config paths: recview's use `HOME`, which Windows doesn't have.
- Warnings in recview's window: on Windows release builds `eprintln` goes nowhere.
- rec2vid: clean up temporary files when it fails, go on without audio when the sounds
  can't be loaded, and show progress only on a terminal.
- Fewer copies across the wasm boundary, and an options object for `setLevelOptions`.
