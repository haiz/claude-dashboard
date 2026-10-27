# Burn-animal animation assets

Source and build scripts for the animated burn-rate animals drawn inside the
5-hour usage ring (52 px compact gauge, 68 px full gauge). Blender is a
build-time tool only: the apps ship the PNG sprite sheets and `manifest.json`,
never Blender or anything from this directory.

| File | Role |
|---|---|
| `BLENDER_VERSION` | The exact first line of `blender --version` used for final renders. `render.py` refuses any other Blender. |
| `build_scene.py` | Reviewable source of `burn_animals.blend`: builds one scene per animal (rig, parts, materials, camera, keyed actions). |
| `burn_animals.blend` | The generated Blender project the sheets are rendered from (binary; regenerate it, do not hand-edit it). |
| `common.py` | Constants shared by the two Blender scripts (ids, frame count, cell size, camera). |
| `render.py` | Renders the `.blend` into `<id>.png` sheets and `manifest.json`. |
| `validate_assets.py` | Checks sheets, manifest and app copies against the asset contract. Pillow only, no Blender. |
| `preview.py` | Writes review strips of every frame at 52 px and 68 px on light and dark backgrounds. Pillow only. |

## Contract

- Ids and files: `snail`, `turtle`, `rabbit`, `horse`, `cheetah` (ladder order),
  each `<id>.png`. File names never use emoji.
- Sheet: 768 x 512 px RGBA PNG, a 6 x 4 grid of 128 x 128 px cells, frames
  0..23 row-major from the top-left cell. At most 2 MiB.
- Timing: a 2-second loop, 24 frames at 12 fps. Frame 24 would equal frame 0, so
  23 -> 0 is seamless. Frame 0 is a calm pose (the app holds it while the animal
  enters).
- Framing: side view, facing right, running in place (no travel across the
  cell). Every frame's non-transparent bounding box lies inside the centred
  112 x 112 px subject box with its centre within 8 px of the cell centre;
  everything outside that box is fully transparent.
- Look: a dark outline for light themes plus a faint light rim outside it for
  dark themes.
- `manifest.json`: `schemaVersion`, `frameCount`, `framesPerSecond`, `columns`,
  `rows`, `cellWidth`, `cellHeight`, `sheetWidth`, `sheetHeight`, `frameOrder`,
  `blenderVersion`, `sourceRevision` (`sha256:` of `burn_animals.blend`) and
  `animals` (`id`, relative `sheet`).

## Motion

Poses are closed-form periodic functions of the loop phase, keyed on every
frame. Legs use two-bone IK against a fixed ground line, so a planted foot
stays on the ground and slides back at the same treadmill speed as every other
planted foot.

| Animal | Cycles per loop | Motion |
|---|---|---|
| snail | 1 | foot stretches and contracts with a forward pedal ripple; shell bobs and tilts; eye stalks sway with lag |
| turtle | 2 | diagonal-pair walk; shell dips at each mid-stance and rocks fore/aft; head bobs |
| rabbit | 2 | crouch, push-off with the heel peeling up, stretched flight, forepaws land, hind feet land ahead of them; spine flexes; ears trail and flop |
| horse | 3 | four-beat transverse gallop with one gathered suspension; body pitches, neck counter-rotates; tail follows with lag |
| cheetah | 4 | rotary gallop with extended and gathered suspensions; spine flexes, head stays level, tail counterbalances the hips |

## Build, render, validate

From this directory, with `B` set to the Blender named in `BLENDER_VERSION`
(for example `B=~/Applications/Blender.app/Contents/MacOS/Blender`):

```sh
# 1. Regenerate the .blend (only after editing build_scene.py).
$B --background --factory-startup --python-exit-code 1 \
   --python build_scene.py -- --output burn_animals.blend

# 2. Render the sheets and manifest into this directory (~25 s).
$B --background --factory-startup burn_animals.blend \
   --python-exit-code 1 --python render.py -- --out .

# 3. Review every frame at gauge size on light and dark backgrounds.
python3 -I preview.py . /tmp/burn-animal-preview

# 4. Copy the exact files into both apps.
for d in ../../apps/macos/ClaudeDashboard/Resources/BurnAnimals \
         ../../apps/windows/app/ui/assets/burn-animals; do
  mkdir -p "$d" && cp manifest.json snail.png turtle.png rabbit.png horse.png cheetah.png "$d"/
done

# 5. Validate the originals and require byte-identical app copies.
python3 -I validate_assets.py . \
   ../../apps/macos/ClaudeDashboard/Resources/BurnAnimals \
   ../../apps/windows/app/ui/assets/burn-animals
```

`validate_assets.py` exits 1 and prints one `FAIL <animal>: frame <n> ...`
line per violation; it also prints each sheet's largest bounding box, largest
centre offset and frame 23 -> 0 drift.

## Determinism

`render.py` sets every pixel-affecting setting itself rather than trusting the
`.blend`: Workbench engine, 16 AA samples, the bundled `outdoor.sl` studio
light, no shadows/cavity/DOF, Standard view transform on sRGB, 512 x 512 px
transparent film, the orthographic side camera, and frames 0..23. Workbench has
no random sampling. Frames are outlined and box-filtered with numpy, and the
PNGs are written by a fixed encoder (zlib level 9, filter 0, no metadata), so
the same `.blend` gives byte-identical sheets. Rebuilding the `.blend` from
`build_scene.py` gives the same pixels; only the file's bytes (and so
`sourceRevision`) change.

Workbench renders on the GPU. Identical output was verified on one machine
(Apple M1 Pro, macOS); a different GPU or driver could round antialiased edge
pixels differently. Treat the committed sheets as the reference and re-render
only when the art changes.
