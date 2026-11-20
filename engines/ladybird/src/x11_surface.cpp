#include "x11_surface.h"

#if !defined(_WIN32)

#    include <X11/Xlib.h>
#    include <X11/Xutil.h>

#    include <cstdlib>
#    include <cstring>
#    include <vector>

struct KkX11Surface {
    Display* display { nullptr };
    Window window { 0 };
    int x {}, y {}, w { 800 }, h { 600 };
};

KkX11Surface* kk_x11_surface_create(int x, int y, int w, int h)
{
    auto* s = new KkX11Surface;
    s->x = x;
    s->y = y;
    s->w = w > 0 ? w : 1;
    s->h = h > 0 ? h : 1;
    s->display = XOpenDisplay(nullptr);
    if (!s->display) {
        delete s;
        return nullptr;
    }
    int screen = DefaultScreen(s->display);
    Window root = RootWindow(s->display, screen);
    s->window = XCreateSimpleWindow(s->display, root, s->x, s->y, static_cast<unsigned>(s->w),
        static_cast<unsigned>(s->h), 0, BlackPixel(s->display, screen), WhitePixel(s->display, screen));
    XStoreName(s->display, s->window, "Kakangkung · LibWeb");
    XSelectInput(s->display, s->window, ExposureMask | StructureNotifyMask);
    XMapWindow(s->display, s->window);
    XFlush(s->display);
    return s;
}

void kk_x11_surface_destroy(KkX11Surface* s)
{
    if (!s)
        return;
    if (s->display && s->window)
        XDestroyWindow(s->display, s->window);
    if (s->display)
        XCloseDisplay(s->display);
    delete s;
}

void kk_x11_surface_set_bounds(KkX11Surface* s, int x, int y, int w, int h)
{
    if (!s)
        return;
    s->x = x;
    s->y = y;
    s->w = w > 0 ? w : 1;
    s->h = h > 0 ? h : 1;
    if (s->display && s->window)
        XMoveResizeWindow(s->display, s->window, s->x, s->y, static_cast<unsigned>(s->w), static_cast<unsigned>(s->h));
}

void kk_x11_surface_set_visible(KkX11Surface* s, int visible)
{
    if (!s || !s->display || !s->window)
        return;
    if (visible)
        XMapWindow(s->display, s->window);
    else
        XUnmapWindow(s->display, s->window);
    XFlush(s->display);
}

void kk_x11_surface_blit(KkX11Surface* s, int width, int height, std::uint32_t const* pixels)
{
    if (!s || !s->display || !s->window || !pixels || width <= 0 || height <= 0)
        return;
    XImage image {};
    image.width = width;
    image.height = height;
    image.format = ZPixmap;
    image.data = reinterpret_cast<char*>(const_cast<std::uint32_t*>(pixels));
    image.byte_order = LSBFirst;
    image.bitmap_unit = 32;
    image.bitmap_bit_order = LSBFirst;
    image.bitmap_pad = 32;
    image.depth = 24;
    image.bytes_per_line = width * 4;
    image.bits_per_pixel = 32;
    image.red_mask = 0xff0000;
    image.green_mask = 0x00ff00;
    image.blue_mask = 0x0000ff;
    ::GC gc = DefaultGC(s->display, DefaultScreen(s->display));
    XPutImage(s->display, s->window, gc, &image, 0, 0, 0, 0, static_cast<unsigned>(width), static_cast<unsigned>(height));
    XFlush(s->display);
}

#else

struct KkX11Surface { };

KkX11Surface* kk_x11_surface_create(int, int, int, int) { return nullptr; }
void kk_x11_surface_destroy(KkX11Surface*) { }
void kk_x11_surface_set_bounds(KkX11Surface*, int, int, int, int) { }
void kk_x11_surface_set_visible(KkX11Surface*, int) { }
void kk_x11_surface_blit(KkX11Surface*, int, int, std::uint32_t const*) { }

#endif
