#!/usr/bin/env python3
"""Exercise consent and real UPnP SOAP actions against native media workers."""
import argparse,json,os,select,socket,subprocess,tempfile,time,urllib.request
from pathlib import Path
from http.server import ThreadingHTTPServer,SimpleHTTPRequestHandler
from threading import Thread
parser=argparse.ArgumentParser();parser.add_argument('--binaries',required=True);parser.add_argument('--upnp',required=True);args=parser.parse_args()
binaries=Path(args.binaries).resolve()
with tempfile.TemporaryDirectory(prefix='framely-media-test-') as directory:
    root=Path(directory);events=socket.socket(socket.AF_UNIX,socket.SOCK_DGRAM);events.bind(str(root/'events.sock'));events.settimeout(5)
    token='0123456789abcdef0123456789abcdef';env=dict(os.environ,GST_PLUGIN_PATH=str(binaries),FRAMELY_CAST_DIR=directory,FRAMELY_CAST_ID=token)
    airplay=subprocess.Popen([str(binaries/'uxplay'),'-n','Framely test','-vs','framelyvideosink','-as','framelyaudiosink'],env=env,stdout=subprocess.PIPE,stderr=subprocess.STDOUT)
    try:
        time.sleep(2);assert airplay.poll() is None,airplay.stdout.read().decode()
        print('PASS: UxPlay starts with bundled decoders and consent sinks')
    finally:
        airplay.terminate();airplay.wait(timeout=3)
    p=subprocess.Popen(['gst-launch-1.0','-q','videotestsrc','is-live=true','!','video/x-raw,width=320,height=240,framerate=10/1','!','framelyvideosink'],env=env,stderr=subprocess.PIPE)
    try:
        event=json.loads(events.recv(4096));assert event['event']=='video' and event['width']==320,event
        frame=root/(token+'.frame');time.sleep(.2);assert not frame.exists(),'video visible before consent'
        (root/(token+'.accept')).touch();time.sleep(.3);assert frame.stat().st_size==16+320*240*4
    finally:
        p.terminate();p.wait(timeout=3)
        assert not p.stderr.read(), 'GStreamer consent sink emitted errors'
    print('PASS: video consent gate')
    # Capture only a temporary null sink, not the user's active audio output.
    import array
    sink_name=f'framely_cast_test_{os.getpid()}'
    module=subprocess.check_output(['pactl','load-module','module-null-sink',f'sink_name={sink_name}','rate=48000','channels=2'],text=True).strip()
    env['PULSE_SINK']=sink_name
    audio_token='abcdef0123456789abcdef0123456789ab'
    # Frame spatializes ordinary streams; this test-only node name keeps its
    # own temporary sink route intact without changing system settings.
    audio_env=dict(env,FRAMELY_CAST_ID=audio_token,PULSE_PROP='node.name=framely-test-filter-chain')
    p=subprocess.Popen(['gst-launch-1.0','-q','audiotestsrc','is-live=true','wave=sine','!','audio/x-raw,rate=48000,channels=2','!','framelyaudiosink'],env=audio_env,stderr=subprocess.PIPE)
    def peak():
        raw=subprocess.check_output(['ffmpeg','-hide_banner','-loglevel','error','-f','pulse','-i',sink_name+'.monitor','-t','0.2','-acodec','pcm_s16le','-f','s16le','pipe:1'])
        samples=array.array('h');samples.frombytes(raw);return max(map(abs,samples),default=0)
    try:
        event=json.loads(events.recv(4096))
        while event['id']!=audio_token:event=json.loads(events.recv(4096))
        assert event['event']=='audio',event
        time.sleep(.3);assert peak()<10,'audio audible before consent'
        (root/(audio_token+'.accept')).touch();time.sleep(.3);assert peak()>1000,'audio not audible after consent'
        (root/(audio_token+'.accept')).unlink();time.sleep(.3);assert peak()<10,'audio continues after pause'
        print('PASS: audio consent gate and pause')
    finally:
        p.terminate();p.wait(timeout=3);subprocess.run(['pactl','unload-module',module],check=True)
        assert not p.stderr.read(),'GStreamer audio sink emitted errors'

    # A complete renderer description, with the same XML shipped by Framely.
    xml=root/'upnp';xml.mkdir()
    import shutil
    for source in Path(args.upnp).glob('*.xml'):shutil.copyfile(source,xml/source.name)
    services=''.join(f'<service><serviceType>urn:schemas-upnp-org:service:{s}:1</serviceType><serviceId>urn:upnp-org:serviceId:{s}</serviceId><SCPDURL>/{s}.xml</SCPDURL><controlURL>/{s}/control</controlURL><eventSubURL>/{s}/event</eventSubURL></service>' for s in ['AVTransport','RenderingControl','ConnectionManager'])
    (xml/'device.xml').write_text(f'<root xmlns="urn:schemas-upnp-org:device-1-0"><specVersion><major>1</major><minor>0</minor></specVersion><device><deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType><friendlyName>Framely test</friendlyName><manufacturer>Framely</manufacturer><modelName>Test</modelName><UDN>uuid:56ce7555-6baf-4360-bbd6-61a32b06b6aa</UDN><serviceList>{services}</serviceList></device></root>')
    subprocess.run(['ffmpeg','-hide_banner','-loglevel','error','-f','lavfi','-i','testsrc2=size=320x240:rate=10','-t','8','-c:v','libx264','-pix_fmt','yuv420p','-movflags','+faststart',str(root/'video.mp4')],check=True)
    class Handler(SimpleHTTPRequestHandler):
        def __init__(self,*a,**k):super().__init__(*a,directory=directory,**k)
        def log_message(self,*a):pass
        def send_head(self):
            # Real media servers support HTTP Range; SimpleHTTPRequestHandler does not.
            file=Path(self.translate_path(self.path)).open('rb');size=os.fstat(file.fileno()).st_size
            value=self.headers.get('Range');self.remaining=size
            if value:
                import re
                match=re.fullmatch(r'bytes=(\d+)-(\d*)',value);assert match,value
                start=int(match[1]);end=min(int(match[2]) if match[2] else size-1,size-1)
                assert 0<=start<=end<size
                self.send_response(206);self.send_header('Content-Range',f'bytes {start}-{end}/{size}')
                file.seek(start);self.remaining=end-start+1
            else:self.send_response(200)
            self.send_header('Accept-Ranges','bytes');self.send_header('Content-Type',self.guess_type(str(file.name)))
            self.send_header('Content-Length',str(self.remaining));self.end_headers();return file
        def copyfile(self,source,target):
            while self.remaining:
                chunk=source.read(min(65536,self.remaining))
                if not chunk:break
                target.write(chunk);self.remaining-=len(chunk)
    http=ThreadingHTTPServer(('0.0.0.0',0),Handler);Thread(target=http.serve_forever,daemon=True).start()
    p=subprocess.Popen([str(binaries/'framely-receiver'),directory,str(xml),'1'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,bufsize=1,env=env)
    from queue import Queue
    replies=Queue()
    def reader():
        for line in p.stdout:replies.put(line)
    Thread(target=reader,daemon=True).start()
    def read():
        return json.loads(replies.get(timeout=10))
    try:
        ready=read();assert ready['event']=='ready',ready
        host=ready['host'];port=ready['port']
        uri=f'http://{host}:{http.server_port}/video.mp4'
        from xml.sax.saxutils import escape
        def soap(action,fields,service='AVTransport'):
            content=''.join(f'<{k}>{escape(str(v))}</{k}>' for k,v in fields.items())
            urn=f'urn:schemas-upnp-org:service:{service}:1'
            body=f'<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/"><s:Body><u:{action} xmlns:u="{urn}">{content}</u:{action}></s:Body></s:Envelope>'
            req=urllib.request.Request(f'http://{host}:{port}/{service}/control',data=body.encode(),headers={'Content-Type':'text/xml; charset=utf-8','SOAPAction':f'"{urn}#{action}"'})
            try:
                with urllib.request.build_opener(urllib.request.ProxyHandler({})).open(req,timeout=5) as r:
                    assert r.status==200
                    return r.read().decode()
            except urllib.error.HTTPError as e:raise AssertionError(e.read().decode())
        sequence=0
        def control(kind,id,**fields):
            global sequence
            sequence+=1
            p.stdin.write(json.dumps(dict(kind=kind,id=id,request=sequence,**fields))+'\n');p.stdin.flush()
            while True:
                reply=read()
                if reply.get('request')==sequence:
                    assert 'error' not in reply,reply
                    return reply['result']
        soap('SetAVTransportURI',dict(InstanceID=0,CurrentURI=uri,CurrentURIMetaData=''))
        request=read()
        while request['event']=='ready':request=read()
        assert request['event']=='request',request
        id=request['id'];frame=root/(id+'.frame');time.sleep(.2);assert not frame.exists(),'DLNA media played before consent'
        p.stdin.write(json.dumps({'kind':'accept','id':id,'request':1})+'\n');p.stdin.flush()
        response=read();assert response['result'] is True,response
        event=json.loads(events.recv(4096))
        while event['id']!=id:event=json.loads(events.recv(4096))
        assert event['width']==320,event
        time.sleep(.2);assert frame.stat().st_size==16+320*240*4
        status=control('status',id)
        assert 7.9<=status['duration']<=8.1,status
        control('pause',id,paused=True);assert control('status',id)['paused'] is True
        soap('SetVolume',dict(InstanceID=0,Channel='Master',DesiredVolume=37),'RenderingControl')
        assert '<CurrentVolume>37</CurrentVolume>' in soap('GetVolume',dict(InstanceID=0,Channel='Master'),'RenderingControl')
        assert abs(control('status',id)['volume']-.37)<.001
        soap('SetMute',dict(InstanceID=0,Channel='Master',DesiredMute=1),'RenderingControl')
        assert '<CurrentMute>1</CurrentMute>' in soap('GetMute',dict(InstanceID=0,Channel='Master'),'RenderingControl')
        control('seek',id,seconds=2);time.sleep(.15)
        assert control('status',id)['position']>=1.9
        assert '<TrackDuration>00:00:08</TrackDuration>' in soap('GetPositionInfo',dict(InstanceID=0))
        # A second request must leave the first session and frame available.
        soap('SetAVTransportURI',dict(InstanceID=0,CurrentURI=uri,CurrentURIMetaData=''))
        request=read()
        while request.get('event')!='request':request=read()
        second=request['id'];control('accept',second)
        deadline=time.monotonic()+5
        while not (root/(second+'.frame')).exists() and time.monotonic()<deadline:time.sleep(.05)
        assert (root/(second+'.frame')).exists() and frame.exists()
        assert control('status',id)['paused'] is True
        control('stop',second);control('stop',id)
        assert not (root/(id+'.accept')).exists()
        assert '<CurrentTransportState>NO_MEDIA_PRESENT</CurrentTransportState>' in soap('GetTransportInfo',dict(InstanceID=0))
        # Repeated requests/rejections exercise session reclamation beyond the cap.
        for _ in range(18):
            soap('SetAVTransportURI',dict(InstanceID=0,CurrentURI=uri,CurrentURIMetaData=''))
            request=read()
            while request.get('event')!='request':request=read()
            control('reject',request['id'])
        print('PASS: DLNA consent, decoding, pause, seek, volume, concurrent sessions and reclamation')
        # A separate renderer exercises GUPnP discovery and outgoing SOAP calls.
        target_dir=root/'target';target_dir.mkdir();target_xml=root/'target-upnp';shutil.copytree(xml,target_xml)
        target_udn='uuid:084442d0-ed68-427b-8dc8-ae4b310f6830'
        description=(target_xml/'device.xml').read_text().replace('uuid:56ce7555-6baf-4360-bbd6-61a32b06b6aa',target_udn)
        (target_xml/'device.xml').write_text(description)
        target=subprocess.Popen([str(binaries/'framely-receiver'),str(target_dir),str(target_xml),'1'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,bufsize=1,env=env)
        target_replies=Queue()
        def target_reader():
            for line in target.stdout:target_replies.put(json.loads(line))
        Thread(target=target_reader,daemon=True).start()
        try:
            deadline=time.monotonic()+12
            while True:
                found=control('devices','')
                if any(d['id']==target_udn for d in found):break
                assert time.monotonic()<deadline,found
                time.sleep(.2)
            control('send',target_udn,uri=uri)
            while True:
                incoming=target_replies.get(timeout=10)
                if incoming.get('event')=='request':break
            assert incoming['uri']==uri,incoming
            assert not (target_dir/(incoming['id']+'.frame')).exists()
            control('send.stop',target_udn)
            while target_replies.get(timeout=10).get('event')!='ended':pass
            print('PASS: DLNA renderer discovery, outgoing SetAVTransportURI/Play and Stop')
        finally:
            target.terminate();target.wait(timeout=3)
            errors=target.stderr.read();assert 'CRITICAL' not in errors and 'WARNING' not in errors,errors

    finally:
        p.terminate();p.wait(timeout=3);http.shutdown();errors=p.stderr.read();assert 'CRITICAL' not in errors and 'WARNING' not in errors,errors
