#!/usr/bin/env python3
"""Run the panel helper as root and capture workers as the Steam user."""
import argparse,json,os,pwd,signal,socket,subprocess,tempfile,time
from pathlib import Path
parser=argparse.ArgumentParser();parser.add_argument('--binaries',default='/tmp/framely-cast-build/bin');parser.add_argument('--user',default='steamos');args=parser.parse_args()
assert os.geteuid()==0,'Run this test with sudo; only the DRM helper needs root.'
user=pwd.getpwnam(args.user);binaries=Path(args.binaries).resolve()
helper_path=binaries/'framely-panel-grab'
if not helper_path.is_file():
    # Release packages keep the privileged helper in bin and media workers in lib/media.
    helper_path=binaries.parent.parent/'bin/framely-panel-grab'
assert helper_path.is_file(),f'Panel helper not found: {helper_path}'
with tempfile.TemporaryDirectory(prefix='framely-panel-test-') as directory:
    os.chown(directory,user.pw_uid,user.pw_gid)
    for name,options in [('eye',[]),('cropped',['--fov','55','--center-x','10','--center-y','-5']),('raw',['--raw'])]:
        a,b=socket.socketpair()
        helper=subprocess.Popen([str(helper_path)],stdin=a,stderr=subprocess.PIPE);a.close()
        env=dict(os.environ,HOME=user.pw_dir,XDG_CONFIG_HOME=user.pw_dir+'/.config',XDG_RUNTIME_DIR=f'/run/user/{user.pw_uid}',FRAMELY_GRAB_FD=str(b.fileno()))
        def demote():os.initgroups(user.pw_name,user.pw_gid);os.setgid(user.pw_gid);os.setuid(user.pw_uid)
        path=Path(directory)/(name+'.h264')
        with path.open('wb') as output:
            capture=subprocess.Popen([str(binaries/'framely-capture'),'--width','640','--height','360','--fps','30','--bitrate','3','--eye','1',*options],stdout=output,stderr=subprocess.PIPE,env=env,preexec_fn=demote,pass_fds=[b.fileno()]);b.close()
            time.sleep(3);capture.send_signal(signal.SIGTERM);capture.wait(timeout=5)
        helper.wait(timeout=5)
        assert path.stat().st_size>10000,capture.stderr.read().decode()+helper.stderr.read().decode()
        probe=subprocess.run(['ffprobe','-v','error','-framerate','30','-count_frames','-show_entries','stream=width,height,codec_name,nb_read_frames','-of','json',str(path)],capture_output=True,text=True,check=True)
        stream=json.loads(probe.stdout)['streams'][0];assert stream['width']==640 and stream['codec_name']=='h264' and int(stream['nb_read_frames'])>=10,stream
        print(name,stream)
    print('PASS: panel capture, Iris encoding, cropped eye view and raw stereo')
