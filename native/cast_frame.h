#pragma once
#include <sys/file.h>
#include <sys/stat.h>
#include <fcntl.h>
#include <unistd.h>
// Raw decoded video is uploaded once into a GL texture, then scaled on the GPU.
struct CastFrame {
 std::string path;GLuint texture=0,composite=0;int composite_width=0,composite_height=0;uint32_t width=0,height=0;timespec stamp{};std::vector<uint8_t> data;
 bool update(){if(path.empty())return false;int fd=open(path.c_str(),O_RDONLY|O_CLOEXEC|O_NOFOLLOW);if(fd<0)return false;
  struct stat st{};bool changed=fstat(fd,&st)==0&&(st.st_mtim.tv_sec!=stamp.tv_sec||st.st_mtim.tv_nsec!=stamp.tv_nsec);
  if(!changed||flock(fd,LOCK_SH|LOCK_NB)){close(fd);return false;}
  uint32_t header[4]{};bool valid=pread(fd,header,16,0)==16&&header[0]==0x46434153&&header[1]>0&&header[2]>0&&header[1]<=4096&&header[2]<=4096&&header[3]==header[1]*4&&st.st_size==16+int64_t(header[1])*header[2]*4;
  if(valid){data.resize(size_t(header[1])*header[2]*4);valid=pread(fd,data.data(),data.size(),16)==ssize_t(data.size());}
  flock(fd,LOCK_UN);close(fd);if(!valid)return false;
  stamp=st.st_mtim;width=header[1];height=header[2];if(!texture)glGenTextures(1,&texture);glBindTexture(GL_TEXTURE_2D,texture);glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_MIN_FILTER,GL_LINEAR);glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_MAG_FILTER,GL_LINEAR);glPixelStorei(GL_UNPACK_ROW_LENGTH,0);glPixelStorei(GL_UNPACK_ALIGNMENT,4);glTexImage2D(GL_TEXTURE_2D,0,GL_RGBA8,width,height,0,GL_BGRA,GL_UNSIGNED_BYTE,data.data());return true;
 }
 GLuint draw(GLuint target,int target_width,int target_height){if(!texture||target_height<=0)return target;
  static auto gen=(PFNGLGENFRAMEBUFFERSPROC)glXGetProcAddressARB((const GLubyte*)"glGenFramebuffers");
  static auto bind=(PFNGLBINDFRAMEBUFFERPROC)glXGetProcAddressARB((const GLubyte*)"glBindFramebuffer");
  static auto attach=(PFNGLFRAMEBUFFERTEXTURE2DPROC)glXGetProcAddressARB((const GLubyte*)"glFramebufferTexture2D");
  static auto blit=(PFNGLBLITFRAMEBUFFERPROC)glXGetProcAddressARB((const GLubyte*)"glBlitFramebuffer");
  static auto del=(PFNGLDELETEFRAMEBUFFERSPROC)glXGetProcAddressARB((const GLubyte*)"glDeleteFramebuffers");
  if(!gen||!bind||!attach||!blit||!del)return target;
  if(!composite)glGenTextures(1,&composite);glBindTexture(GL_TEXTURE_2D,composite);if(composite_width!=target_width||composite_height!=target_height){glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_MIN_FILTER,GL_LINEAR);glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_MAG_FILTER,GL_LINEAR);glTexImage2D(GL_TEXTURE_2D,0,GL_RGBA8,target_width,target_height,0,GL_BGRA,GL_UNSIGNED_BYTE,nullptr);composite_width=target_width;composite_height=target_height;}GLuint fbo[2];gen(2,fbo);bind(GL_READ_FRAMEBUFFER,fbo[0]);attach(GL_READ_FRAMEBUFFER,GL_COLOR_ATTACHMENT0,GL_TEXTURE_2D,texture,0);bind(GL_DRAW_FRAMEBUFFER,fbo[1]);attach(GL_DRAW_FRAMEBUFFER,GL_COLOR_ATTACHMENT0,GL_TEXTURE_2D,composite,0);// Decoder frames are top-down; CEF controls are already flipped for GL.
  blit(0,height,width,0,0,0,target_width,target_height,GL_COLOR_BUFFER_BIT,GL_LINEAR);
  // CEF paints transparent, premultiplied controls over the full video plane.
  // Keep its texture intact so partial CEF damage never contains stale video.
  bind(GL_FRAMEBUFFER,fbo[1]);glPushAttrib(GL_ALL_ATTRIB_BITS);glViewport(0,0,target_width,target_height);glDisable(GL_DEPTH_TEST);glDisable(GL_SCISSOR_TEST);glEnable(GL_TEXTURE_2D);glEnable(GL_BLEND);glBlendFunc(GL_ONE,GL_ONE_MINUS_SRC_ALPHA);glColor4f(1,1,1,1);glBindTexture(GL_TEXTURE_2D,target);
  glMatrixMode(GL_PROJECTION);glPushMatrix();glLoadIdentity();glOrtho(0,1,0,1,-1,1);glMatrixMode(GL_MODELVIEW);glPushMatrix();glLoadIdentity();glBegin(GL_QUADS);glTexCoord2f(0,0);glVertex2f(0,0);glTexCoord2f(1,0);glVertex2f(1,0);glTexCoord2f(1,1);glVertex2f(1,1);glTexCoord2f(0,1);glVertex2f(0,1);glEnd();glPopMatrix();glMatrixMode(GL_PROJECTION);glPopMatrix();glPopAttrib();bind(GL_FRAMEBUFFER,0);del(2,fbo);return composite;
 }
 ~CastFrame(){if(texture)glDeleteTextures(1,&texture);if(composite)glDeleteTextures(1,&composite);}
};
