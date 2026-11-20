#pragma once

#include <cstdint>

// Thin X11 surface used by the LibWeb host on Linux. Kept out of Ladybird TUs so
// X11's GC typedef does not collide with LibGC::GC.

struct KkX11Surface;

KkX11Surface* kk_x11_surface_create(int x, int y, int w, int h);
void kk_x11_surface_destroy(KkX11Surface* s);
void kk_x11_surface_set_bounds(KkX11Surface* s, int x, int y, int w, int h);
void kk_x11_surface_set_visible(KkX11Surface* s, int visible);
/// Blit BGRA/XRGB 32-bit pixels (top-down) into the window.
void kk_x11_surface_blit(KkX11Surface* s, int width, int height, std::uint32_t const* pixels);
