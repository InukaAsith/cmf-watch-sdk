# Publishing an app to the FMC store

The [FMC store](https://fmc.freethinkel.dev/apps) lists apps from this repository. An app opts in with a
`store.toml` next to its `Cargo.toml`; apps without one are never published.

1. Write the app as `docs/app-guide.md` describes, in `sdk/examples/<name>/`. The crate name is the package name
   and the store's key for the app: at most 23 bytes, lowercase letters, digits, `-` and `_`.
2. Add `sdk/examples/<name>/store.toml` and `store.md` (below). The app's `icon.svg` becomes the store icon too,
   rendered to a 288 px PNG.
3. Open a pull request. The **Store** workflow builds every store app the PR touches, checks each package with
   `wp2apps info` and attaches the packages to the run as the `store-apps` artifact. Install one in the emulator or,
   with your own watch, on it, before asking for a review.
4. After the merge to `main` the workflow builds the app again and publishes it. Nothing else is needed.

## store.toml

```toml
name = "Vibration Lab"                                # the store's title, at most 60 characters
summary = "Tune the motor: strength, frequency, patterns"  # one line under the title, at most 140
version = "0.1.0"                                     # shown on the page, at most 24 characters
build = 1                                             # the package's version number in the .wp2app header
author = "CMF Tech Community"                         # at most 60 characters
published = true                                      # false: uploaded, but hidden from the store
screenshots = ["shots/main.png"]                      # optional, at most 8; png, jpeg, webp or svg, 2 MB each
```

`name` and `build` are required. Bump `build` with every change to the app's code.

`store.md` is the app page's body, Markdown, at most 8000 characters. Say what the app does, then what it needs
under `## Requirements` (the patched firmware, the App Loader, a companion app). Relative links like
`/firmware` point into the store.

## What the store gets

The listing is the whole truth: a field, icon or screenshot removed here is removed from the store. The store
keeps the record, its id and its download count. It replaces the package only when the header CRC changes, so
editing `store.md` alone doesn't produce a new download. The source link points at `sdk/examples/<name>` on
`main`, the ABI comes from the package header.

The store checks the package as `wp2apps info` does and rejects one whose header name isn't the crate name.

A change to `sdk/cmf-wp2-app`, `sdk/cmf-wp2-ui`, the workspace files or the build tooling rebuilds and republishes
every store app. A maintainer can run the workflow by hand (Actions, **Store**, *Run workflow*), with a dry run that
only asks the store what would change.

## For maintainers

The publish job sends each package to `POST /api/store/publish` (`fmc_pocketbase`, `pb_hooks/publish.pb.js`) with
the `STORE_PUBLISH_TOKEN` secret; `STORE_URL` is a repository variable. The token can only create or update app
listings. To rotate it, set a new value in the store container's environment and here, with `gh secret set`.
