#!/usr/bin/env python3
"""Exercise the patched UxPlay timestamp retry using its actual video callback."""
from pathlib import Path
import subprocess
import tempfile
import sys

source = Path(sys.argv[1] if len(sys.argv) > 1 else 'media/source/UxPlay-1.73.7') / 'uxplay.cpp'
text = source.read_text()
start = text.index('extern "C" void video_process (')
end = text.index('\n#ifdef DBUS', start)
callback = text[start:end]
harness = r'''
#include <cstdint>
#include <vector>
#include <cassert>
#include <cstdio>
struct raop_ntp_t {};
struct video_decode_struct { unsigned char* data; int data_len; int nal_count; uint64_t ntp_time_remote, ntp_time_local; };
static bool dump_video=false, mux_to_file=false, use_video=true;
static uint64_t remote_clock_offset=0;
static constexpr uint64_t SECOND_IN_NSECS=1000000000;
static std::vector<uint64_t> timestamps;
static std::vector<uint64_t> corrections;
#define LOGI(...) ((void)0)
static uint64_t get_local_time(){return 1793025192274634768ULL;}
static void dump_video_to_file(unsigned char*,int){}
static void mux_renderer_push_video(unsigned char*,int,uint64_t){}
static uint64_t video_renderer_render_buffer(unsigned char*,int*,int*,uint64_t* timestamp){
 timestamps.push_back(*timestamp);
 auto index=timestamps.size()-1;
 return index<corrections.size()?corrections[index]:0;
}
'''
tests = r'''
int main(){
 constexpr uint64_t local=1793025192274634768ULL, remote=61000000000000ULL;
 for(auto retry : {std::vector<uint64_t>{},std::vector<uint64_t>{72640000},std::vector<uint64_t>{72640000,1000000}}){
  timestamps.clear();corrections=retry;remote_clock_offset=0;
  video_decode_struct frame{nullptr,1,1,remote,local};
  video_process(nullptr,nullptr,&frame);
  assert(timestamps.size()==retry.size()+1);
  uint64_t expected=local;
  for(size_t i=0;i<timestamps.size();++i){assert(timestamps[i]==expected);if(i<retry.size())expected+=retry[i];}
  // A later frame must carry the same corrected clock offset, without a stall.
  frame.ntp_time_remote=remote+33333333;
  video_process(nullptr,nullptr,&frame);
  assert(timestamps.back()==expected+33333333);
 }
 puts("PASS: mirror clock offset applied once across zero, one and multiple timestamp corrections");
}
'''
with tempfile.TemporaryDirectory(prefix='framely-airplay-timing-') as directory:
    path = Path(directory)
    (path/'test.cpp').write_text(harness + callback + tests)
    subprocess.run(['g++','-std=c++17','-O2',str(path/'test.cpp'),'-o',str(path/'test')],check=True)
    subprocess.run([str(path/'test')],check=True)
