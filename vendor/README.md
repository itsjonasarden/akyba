# Vendored GPUI

`gpui-pre`, `gpui-pre-apple` and `gpui-pre-macos` 0.3.9 from crates.io (Apache-2.0, see each
crate's `LICENSE-APACHE`), with four patches: drop shadows that stay outside their element;
translucent layers that stack instead of adding up; lists that keep their items' heights as hints
when their width changes, so a diff review scrolls; and glyphs sorted by small keys instead of
moving each 112-byte glyph, which is cheaper on most frames.
The root `Cargo.toml` points Akyba at these copies with `[patch.crates-io]`; the workspace
excludes them, so Akyba's lints and formatter leave them alone. `gpui-pre`'s examples were left
out.

Every change is marked "Akyba's patch":

- `gpui-pre/src/scene.rs`: monochrome glyphs are sorted by cached keys, in the same stable paint
  order, and a frame already in order isn't sorted at all. The same file is copied to
  `gpui-pre-apple/vendor/gpui/src/`, where the build script reads it to generate the shader
  header. The test that holds it to the original order, and a benchmark to run by hand, are in
  `crates/akyba-terminal/tests/scene_sort.rs`.
- `gpui-pre-apple/src/metal_renderer.rs`: the pipelines blend alpha "over"
  (`OneMinusSourceAlpha`) instead of adding it.
- `gpui-pre-apple/src/shaders.metal`: `shadow_fragment` clips a drop shadow to outside its
  element, as CSS does, so shadows don't tint translucent panels.
- `gpui-pre-macos/src/gpui_macos.rs`: `#![allow(deprecated, dead_code)]`, so upstream's use of the
  deprecated `cocoa` crate doesn't print a thousand warnings on every build.
- `gpui-pre/src/elements/list.rs`: when a list's width changes (its first frame included), items
  keep their last height as a hint instead of none. Unmeasured items without a hint count as zero
  high, which wiped the uniform row height gpui-component's diff gives before its first frame, so
  a review couldn't scroll past the rows already drawn.

Akyba once also had a backdrop blur primitive and a patched macOS window blur. Both were removed:
nothing drew the first, the theme's backdrop hid the second, and together they kept the layer off
macOS's fast path and gave WindowServer a blur pass on every frame.

To move to a newer GPUI: copy the new crates here, then re-apply the changes above.
