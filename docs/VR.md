# Virtual reality (Windows)

openOMSI can render through an OpenXR headset on Windows. An active OpenXR runtime
and a DirectX 12 capable graphics adapter are required. VR is off by default.
The normal desktop controls and rendering remain available when VR is off.

## Start and settings

Connect the headset and start its OpenXR runtime before launching a drive. In the
launcher, open **Settings → Camera → Virtual reality** and enable **Use OpenXR headset**.
The game starts in the headset when the session opens.

| Setting | Choices | Effect |
| --- | --- | --- |
| Eye resolution | 50%, 65%, 80%, 100% | Scale of the runtime's recommended resolution for each eye. Lower values reduce GPU work. |
| Head tracking smoothing | Off, 5, 10, 20, 30 ms | Smooth the headset pose; Off uses raw tracking. |
| Bus mirror refresh | Off, 8, 16, 24, 32/s | Limit how often bus mirrors are redrawn in VR; Off freezes their picture. |
| Show headset picture on monitor | On or off | Copy the left eye to the desktop window. |

These settings affect VR only. The game's other graphics settings still apply
and may need adjusting on demanding maps.

## Controls

The VR key bindings are editable under **Controls → Keyboard**; search for `VR`.
The default bindings are:

| Action | Default key |
| --- | --- |
| Reset the VR view | Ctrl+Shift+R |
| Toggle the monitor preview | F7 |
| Switch between VR and desktop | F8 |

Adjust your seating position while driving through **Esc → Options** using
**Seat forward**, **Seat back**, **Seat up**, **Seat down**, **Seat right**, or **Seat left**.
**Reset the seat position** restores the bus camera's default position. The launcher
also has seat-position sliders under **Settings → Camera**; those changes apply when the next
drive starts.

**Esc** opens the menu in front of the headset. Move the mouse to point at cockpit
controls and left-click to use them. The pointer fades after ten seconds without
mouse movement and returns in the centre when moved again. When mouse steering is
active, the cockpit pointer is hidden so the mouse can steer the bus.

In VR, **right-click** toggles a smooth zoom; the next right-click returns to the
normal view. The mouse remains usable while zoomed. Outside VR, right-drag retains
its normal camera control.

The headset runtime controls its own reprojection settings. No runtime debug tool
setting is needed to enable openOMSI's VR mode.
