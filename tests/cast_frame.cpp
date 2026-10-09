#include <GL/gl.h>
#include <GL/glx.h>
#include <GL/glext.h>
#include <X11/Xlib.h>
#include <cstdint>
#include <string>
#include <vector>
#include <cassert>
#include <iostream>
#include "cast_frame.h"
int main(){
 auto* display=XOpenDisplay(nullptr);assert(display);int attrs[]={GLX_RGBA,GLX_RED_SIZE,8,GLX_GREEN_SIZE,8,GLX_BLUE_SIZE,8,GLX_ALPHA_SIZE,8,None};auto* visual=glXChooseVisual(display,DefaultScreen(display),attrs);assert(visual);auto context=glXCreateContext(display,visual,nullptr,True);assert(context);auto pixmap=XCreatePixmap(display,RootWindow(display,visual->screen),4,4,visual->depth);auto surface=glXCreateGLXPixmap(display,visual,pixmap);assert(glXMakeCurrent(display,surface,context));
 {
 CastFrame frame;frame.width=frame.height=2;glGenTextures(1,&frame.texture);glBindTexture(GL_TEXTURE_2D,frame.texture);glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_MIN_FILTER,GL_LINEAR);glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_MAG_FILTER,GL_LINEAR);std::vector<uint8_t> blue(16);for(size_t n=0;n<blue.size();n+=4){blue[n+2]=255;blue[n+3]=255;}glTexImage2D(GL_TEXTURE_2D,0,GL_RGBA8,2,2,0,GL_RGBA,GL_UNSIGNED_BYTE,blue.data());
 GLuint ui;glGenTextures(1,&ui);glBindTexture(GL_TEXTURE_2D,ui);glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_MIN_FILTER,GL_LINEAR);glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_MAG_FILTER,GL_LINEAR);std::vector<uint8_t> controls(64);for(size_t n=0;n<controls.size();n+=4){controls[n]=128;controls[n+3]=128;}glTexImage2D(GL_TEXTURE_2D,0,GL_RGBA8,4,4,0,GL_RGBA,GL_UNSIGNED_BYTE,controls.data());
 auto output=frame.draw(ui,4,4);assert(output!=ui);glBindTexture(GL_TEXTURE_2D,output);std::vector<uint8_t> pixels(64);glGetTexImage(GL_TEXTURE_2D,0,GL_RGBA,GL_UNSIGNED_BYTE,pixels.data());for(size_t n=0;n<pixels.size();n+=4){assert(pixels[n]==128);assert(pixels[n+1]==0);assert(pixels[n+2]>=126&&pixels[n+2]<=128);assert(pixels[n+3]==255);}glBindTexture(GL_TEXTURE_2D,ui);glGetTexImage(GL_TEXTURE_2D,0,GL_RGBA,GL_UNSIGNED_BYTE,pixels.data());assert(pixels==controls);
 std::fill(controls.begin(),controls.end(),0);glTexSubImage2D(GL_TEXTURE_2D,0,0,0,4,4,GL_RGBA,GL_UNSIGNED_BYTE,controls.data());output=frame.draw(ui,4,4);glBindTexture(GL_TEXTURE_2D,output);glGetTexImage(GL_TEXTURE_2D,0,GL_RGBA,GL_UNSIGNED_BYTE,pixels.data());assert(pixels[0]==0&&pixels[2]==255);assert(glGetError()==GL_NO_ERROR);glDeleteTextures(1,&ui);
 }
 glXMakeCurrent(display,None,nullptr);glXDestroyGLXPixmap(display,surface);XFreePixmap(display,pixmap);glXDestroyContext(display,context);XFree(visual);XCloseDisplay(display);std::cout<<"Full-frame video and transparent controls composition passed\n";
}
