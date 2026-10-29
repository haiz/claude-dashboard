"""Constants shared by build_scene.py and render.py (both run inside Blender).

validate_assets.py deliberately does not import this module: it re-states the
numbers it checks so a wrong constant here cannot also pass its own check.
"""

ANIMALS = ("snail", "turtle", "rabbit", "horse", "cheetah", "rocket")  # stable asset ids, ladder order

FRAME_COUNT = 24        # frames 0..23; the pose at frame 24 equals frame 0
FPS = 12
COLUMNS, ROWS = 6, 4    # sheet grid, row-major from the top-left cell
CELL = 128              # cell size in px
SUPERSAMPLE = 4         # frames render at CELL * SUPERSAMPLE, then box-filter down

ORTHO_SCALE = 2.56      # world units across the cell (orthographic)
PX_PER_UNIT = CELL / ORTHO_SCALE   # 50 cell px per world unit
FIT_PX = 104            # largest side of each animal's all-frame bounding box, in cell px

CAMERA_NAME = "SideCamera"
CAMERA_DISTANCE = 10.0  # camera sits on -Y looking at +Y; X is right (forward), Z is up
