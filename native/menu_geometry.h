#pragma once
#include "geometry_math.h"
struct MenuPlacement {vr::HmdMatrix34_t transform{};float width=0,radius=0;};
// Continue the Dock arc at the icon, with the main window's upward direction.
// The endpoint tangent, rather than the popup center, determines orientation.
inline bool menu_placement(const vr::HmdMatrix34_t& button,float button_size,MenuPlacement& out,float radius=0,const vr::HmdMatrix34_t* main=nullptr){
 vr::HmdVector3_t right{{button.m[0][0],0,button.m[2][0]}},up{{0,1,0}};
 if(main){for(int r=0;r<3;r++){right.v[r]=main->m[r][0];up.v[r]=main->m[r][1];}}
 if(!normalize_dock_vector(up)||!std::isfinite(button_size)||button_size<=0)return false;
 if(radius>0){
  // Match the horizontal tangent at the Dock attachment point.
  for(int r=0;r<3;r++)right.v[r]=button.m[r][0];
  float projection=0;for(int r=0;r<3;r++)projection+=right.v[r]*up.v[r];
  for(int r=0;r<3;r++)right.v[r]-=up.v[r]*projection;
 }
 if(!normalize_dock_vector(right))return false;
 auto normal=cross_dock_vector(right,up);if(!normalize_dock_vector(normal))return false;up=cross_dock_vector(normal,right);
 constexpr float width=.32f,height=width*840.f/600.f,gap=.012f;
 float edge_x=width*.5f,edge_z=0;out.radius=radius;
 if(radius>0){
  float angle=width*.5f/radius,c=std::cos(angle),s=std::sin(angle);
  // Move from the right endpoint back along the same circle to its midpoint.
  for(int r=0;r<3;r++){float t=right.v[r],n=normal.v[r];right.v[r]=t*c-n*s;normal.v[r]=n*c+t*s;}
  edge_x=radius*s;edge_z=radius*(1-c);
 }
 for(int r=0;r<3;r++){
  out.transform.m[r][0]=right.v[r];out.transform.m[r][1]=up.v[r];out.transform.m[r][2]=normal.v[r];
  float corner=button.m[r][3]+(button.m[r][1]+button.m[r][0])*button_size*.5f;
  if(radius>0){float a=button_size*.5f/radius;corner=button.m[r][3]+button.m[r][1]*button_size*.5f+button.m[r][0]*radius*std::sin(a)+button.m[r][2]*radius*(1-std::cos(a));}
  out.transform.m[r][3]=corner-right.v[r]*edge_x-normal.v[r]*edge_z+up.v[r]*(height*.5f+gap);
  if(!std::isfinite(out.transform.m[r][3]))return false;
 }
 out.width=width;return true;
}

// The button transform carries the shared circle axis and radial normal.
// Pre-curve pitch slopes only the Dock row; the popup rises along that same axis.
inline bool dock_menu_placement(const vr::HmdMatrix34_t& button,float size,float radius,float dock_pitch,MenuPlacement& out){
 if(!std::isfinite(radius)||radius<=0||!std::isfinite(size)||size<=0||!std::isfinite(dock_pitch))return false;
 constexpr float width=.32f,height=.448f,gap=.012f;
 float h=size*.5f,top_radius=radius-h*std::sin(dock_pitch);
 if(top_radius<=0)return false;
 float angle=h/radius-width*.5f/top_radius,c=std::cos(angle),s=std::sin(angle);
 for(int r=0;r<3;r++){
  float t=button.m[r][0],up=button.m[r][1],n=button.m[r][2];
  float normal=n*c-t*s;
  out.transform.m[r][0]=t*c+n*s;out.transform.m[r][1]=up;out.transform.m[r][2]=normal;
  float circle=button.m[r][3]+n*radius;
  out.transform.m[r][3]=circle-normal*top_radius+up*(h*std::cos(dock_pitch)+gap+height*.5f);
  if(!std::isfinite(out.transform.m[r][3]))return false;
 }
 out.width=width;out.radius=top_radius;return true;
}
