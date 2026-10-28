// Exercise UxPlay's real HTTP handlers without publishing a network service.
#include "raop.c"
#include <math.h>
static int played,reset,stopped;
static float last_start,last_rate,last_seek;
static char *last_url;
static void log_probe(void* cls,int level,const char* message){assert(!strstr(message,"private-token"));}
static void play_probe(void* cls,const char* url,float start){played++;free(last_url);last_url=strdup(url);last_start=start;}
static void reset_probe(void* cls,int reason){reset++;}
static void stop_probe(void* cls){stopped++;}
static void rate_probe(void* cls,float value){last_rate=value;}
static void scrub_probe(void* cls,float value){last_seek=value;}
static http_request_t* request(const char* target,const char* body,int size){
 char header[512];int n=snprintf(header,sizeof(header),"POST %s HTTP/1.1\r\nX-Apple-Session-ID: test-session\r\nContent-Type: application/x-apple-binary-plist\r\nContent-Length: %d\r\n\r\n",target,size);
 http_request_t* r=http_request_init();assert(!http_request_add_data(r,header,n));if(size)assert(!http_request_add_data(r,body,size));assert(http_request_is_complete(r));return r;
}
static void run(const char* url,int app,int direct,int invalid){
 played=reset=stopped=0;raop_t raop={0};raop.logger=logger_init();logger_set_callback(raop.logger,log_probe,NULL);raop.current_video=-1;raop.port=7000;
 raop.callbacks.on_video_play=play_probe;raop.callbacks.conn_reset=reset_probe;raop.callbacks.on_video_stop=stop_probe;raop.callbacks.on_video_rate=rate_probe;raop.callbacks.on_video_scrub=scrub_probe;
 httpd_callbacks_t callbacks={0};callbacks.conn_init=conn_init;callbacks.conn_request=conn_request;callbacks.conn_destroy=conn_destroy;raop.httpd=httpd_init(raop.logger,&callbacks,0);assert(raop.httpd);
 raop_conn_t conn={0};conn.raop=&raop;plist_t p=plist_new_dict();plist_dict_set_item(p,"uuid",plist_new_string("test-video"));plist_dict_set_item(p,"Content-Location",plist_new_string(url));if(app)plist_dict_set_item(p,"clientProcName",plist_new_string("YouTube"));plist_dict_set_item(p,"Start-Position-Seconds",plist_new_real(1.25));
 char* body=NULL;uint32_t size=0;plist_to_bin(p,&body,&size);http_request_t* r=request("/play",body,size);http_response_t* response=http_response_create();http_response_init(response,"HTTP/1.1",200,"OK");char* data=NULL;int length=0;
 http_handler_play(&conn,r,response,&data,&length);
 assert(played==direct);assert(reset==invalid);
 if(direct){
  assert(!strcmp(last_url,url));assert(fabs(last_start-1.25)<.001);
  http_request_destroy(r);r=request("/rate?value=0",NULL,0);http_handler_rate(&conn,r,response,&data,&length);assert(last_rate==0);
  http_request_destroy(r);r=request("/rate?value=1",NULL,0);http_handler_rate(&conn,r,response,&data,&length);assert(last_rate==1);
  http_request_destroy(r);r=request("/scrub?position=2.5",NULL,0);http_handler_scrub(&conn,r,response,&data,&length);assert(fabs(last_seek-2.5)<.001);
  http_request_destroy(r);r=request("/stop",NULL,0);http_handler_stop(&conn,r,response,&data,&length);assert(stopped==1);
 }else if(!invalid){
  assert(raop.current_video>=0);assert(!strcmp(get_playback_location(raop.airplay_video[raop.current_video]),"http://localhost:7000/master.m3u8"));
 }
 raop_destroy_airplay_video(&raop,-1);httpd_destroy(raop.httpd);http_response_destroy(response);http_request_destroy(r);plist_mem_free(body);plist_free(p);logger_destroy(raop.logger);
}
int main(){
 run("https://example.invalid/video.mp4?private-token=redacted",1,1,0);
 run("http://127.0.0.1/video.mp4",0,1,0);
 run("https://example.invalid/video.m3u8",0,1,0);
 run("mlhls://localhost/master.m3u8",1,0,0);
 run("file:///private-token",0,0,1);
 free(last_url);puts("PASS: direct MP4/HLS, optional app name, start position, rate, seek, stop, FCUP preservation and URL privacy");
}
