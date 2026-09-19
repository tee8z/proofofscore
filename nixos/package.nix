{ package }:

# Older pinned server packages install only the executable. Keep this recipe
# with the application so consumers can retain their package revision.
package.overrideAttrs (old: {
  postInstall = (old.postInstall or "") + ''
    mkdir -p $out/share/proofofscore/static $out/share/proofofscore/migrations
    cp -r crates/server/static/. $out/share/proofofscore/static/
    cp -r crates/server/migrations/. $out/share/proofofscore/migrations/
  '';
})
