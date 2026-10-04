#!/usr/bin/python3
import os,pathlib
from framely import serve

data=pathlib.Path(os.environ.get('FRAMELY_DATA_DIR','/tmp/framely-example'))
def initialize(context):
 data.mkdir(parents=True,exist_ok=True)
 (data/'version.txt').write_text(context['version'])
 return {'ready':True}
def stop(context):
 return {'stopped':True}
def dispatch(method,params):
 if method=='identity':
  try:list(pathlib.Path('/home/steamos').iterdir());accessible=True
  except (OSError,PermissionError):accessible=False
  return {'uid':os.getuid(),'gid':os.getgid(),'home':os.environ.get('HOME'),'dataDir':str(data),'steamHomeAccessible':accessible}
 if method=='save':
  text=str(params.get('text',''))[:4096];(data/'note.txt').write_text(text);return {'saved':text}
 if method=='read':return {'text':(data/'note.txt').read_text() if (data/'note.txt').exists() else ''}
 if method=='notification.action':return {'action':params.get('action'),'notification':params.get('id')}
 raise ValueError('Unknown method')
serve(dispatch,{'onInstall':initialize,'onUpdate':initialize,'onStart':initialize,'onStop':stop,'onUninstall':stop,'onCrashCleanup':stop})
