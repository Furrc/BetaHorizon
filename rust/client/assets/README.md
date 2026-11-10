# Client render assets

The Rust client keeps the original Beta 1.7.3 render textures here, extracted
byte-for-byte from `jars/minecraft.jar`:

- `terrain/terrain.png`: 256x256 block atlas, including water/lava source tiles.
- `terrain/water-flowing.png` and `terrain/water-side.png`: exact 16x16 crops
  of original atlas tiles 205 and 206. Water uses a dedicated 32x16 texture
  made from these images so fluid UVs cannot sample unrelated atlas rows.
- `terrain/sun.png` and `terrain/moon.png`: original celestial sprites.
- `environment/clouds.png`: original cloud texture.

All files are embedded by the renderer. `TextureWaterFX` updates only the
flowing-water half of the dedicated water texture; side faces sample tile 206.
