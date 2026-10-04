"""Validate the SDK through real independent-hook and backend processes."""
import json, os, pathlib, subprocess, tempfile, unittest
SDK = pathlib.Path(__file__).parents[1] / 'sdk/python'
SOURCE = """from framely import serve
import pathlib

def callback(ctx):
 pathlib.Path(ctx['dataDir']).joinpath('phase').write_text(ctx['phase'])
 return {'phase':ctx['phase']}
serve(lambda method,params:{'method':method}, {'onInstall':callback,'onStart':callback,'onStop':callback})
"""
class Lifecycle(unittest.TestCase):
 def test_independent_install_runs_once_without_rpc_input(self):
  with tempfile.TemporaryDirectory() as data:
   context={'phase':'onInstall','dataDir':data}
   env={**os.environ,'PYTHONPATH':str(SDK),'FRAMELY_LIFECYCLE':'onInstall','FRAMELY_LIFECYCLE_CONTEXT':json.dumps(context)}
   result=subprocess.run(['python3','-c',SOURCE],env=env,capture_output=True,text=True,timeout=3)
   self.assertEqual(result.returncode,0,result.stderr)
   self.assertEqual(pathlib.Path(data,'phase').read_text(),'onInstall')
   self.assertEqual(result.stdout,'')
 def test_backend_hooks_and_business_requests_share_protocol(self):
  with tempfile.TemporaryDirectory() as data:
   requests=[{'id':1,'method':'framely.lifecycle.start','params':{'phase':'onStart','dataDir':data}},{'id':2,'method':'hello','params':{}},{'id':3,'method':'framely.lifecycle.stop','params':{'phase':'onStop','dataDir':data}}]
   result=subprocess.run(['python3','-c',SOURCE],env={**os.environ,'PYTHONPATH':str(SDK)},input='\n'.join(map(json.dumps,requests))+'\n',capture_output=True,text=True,timeout=3)
   self.assertEqual(result.returncode,0,result.stderr)
   replies=list(map(json.loads,result.stdout.splitlines()))
   self.assertEqual(replies,[{'id':1,'result':{'phase':'onStart'}},{'id':2,'result':{'method':'hello'}},{'id':3,'result':{'phase':'onStop'}}])
   self.assertEqual(pathlib.Path(data,'phase').read_text(),'onStop')
 def test_missing_independent_callback_is_failure(self):
  result=subprocess.run(['python3','-c',SOURCE],env={**os.environ,'PYTHONPATH':str(SDK),'FRAMELY_LIFECYCLE':'onUninstall','FRAMELY_LIFECYCLE_CONTEXT':'{}'},capture_output=True,text=True,timeout=3)
  self.assertEqual(result.returncode,1)
if __name__=='__main__': unittest.main()
