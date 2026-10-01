# Modding beyond OMSI 2

openOMSI reads OMSI 2 content as it is: a bus, a map or an object made for OMSI 2 works
without changes. It also lifts limits OMSI 2 put on modders. Everything on this page is an
addition. A file that uses it still loads in OMSI 2, which ignores what it does not know.

## Interior lights: more than four per mesh

OMSI 2 lights a mesh with at most four `[interiorlight]` lamps, the four numbers of its
`[illumination_interior]`. openOMSI takes as many as you list, up to 63 per mesh. Write the
extra lamp numbers on the lines right after the first four; a blank line ends the list:

```
[mesh]
saloon.o3d

[illumination_interior]
0
1
2
3
4
5
6
7

[matl]
...
```

OMSI 2 reads the first four and skips the rest. A model may declare any number of
`[interiorlight]` lamps, and each mesh names the ones that light it. The same goes for
`[illumination_interior]` in `passengercabin.cfg`.

## Textures of any resolution

- Textures up to 16384 × 16384 pixels load: DDS (DXT1/3/5, uncompressed), TGA, BMP, PNG and
  JPG. Where the graphics card cannot take that size, the texture is halved until it fits.
- There is no 2 GB address-space limit: openOMSI is a 64-bit program.
- A texture keeps its full resolution within 150 m of the camera, so a bus's own 4K
  textures always stay sharp. Far scenery gives up detail only when the texture memory
  (`texture_memory` in the settings, or `OMSI_TEXTURE_MEMORY`, in MB) runs out, as OMSI's
  `[texmemlimit]` does.
- `[scripttexture]`, `[htmltexture]` and `[texttexture]` / `[texttexture_enh]` can be any size the card
  takes.

## PBR materials

Normal, roughness, metalness and occlusion maps beside a texture, up to 4096 × 4096. See
[PBR materials](PBR.md).

## Lights

- Each 25 m square of the world draws up to 32 point and spot lights at once (the nearest
  first), so depots, stations and lit interiors keep their lamps.
- Interior lamps of vehicles are drawn per pixel, with no count limit per vehicle.

## Models

- `.o3d` files with 32-bit indices (the long-index flag) are drawn with their full vertex
  and face count; everything is drawn with 32-bit indices.
- There is no limit on the number of meshes, materials, `[matl_change]` items, `[CTC]`
  entries, cameras, doors, passenger places, wheels or axles.

## Scripts and plugins

- Script variables and string variables have no count limit.
- Lua plugins can read and write script variables, fire triggers and react to game events:
  see [Plugins](PLUGINS.md).

## What stays as in OMSI 2

The following behave as in OMSI 2 so that existing content works unchanged:

- the script stack (8 values) and registers (`l0`-`l9`, `s0`-`s9`);
- one `[spotlight]` lit at a time per vehicle (the one `Spot_Select` picks);
- 100 particles per emitter.

In a LAN session, other players see up to 7 doors, 15 wheels and 127 lamps of a vehicle.
