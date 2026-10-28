#!/usr/bin/env python3
"""Run real UxPlay HTTP handlers with a previously configured Makefile build."""
import argparse
import shlex
import subprocess
import tempfile
from pathlib import Path

parser=argparse.ArgumentParser()
parser.add_argument('source',type=Path)
parser.add_argument('build',type=Path)
args=parser.parse_args()
root=Path(__file__).resolve().parents[1]
source=args.source.resolve();build=args.build.resolve()
flags=(build/'lib/CMakeFiles/airplay.dir/flags.make').read_text()
def values(key):
    return shlex.split(next(line.split(' = ',1)[1] for line in flags.splitlines() if line.startswith(key+' =')))
links=shlex.split((build/'CMakeFiles/uxplay.dir/link.txt').read_text())
libs=links[links.index('renderers/librenderers.a')+1:]
with tempfile.TemporaryDirectory(prefix='framely-airplay-protocol-') as directory:
    work=Path(directory)
    subprocess.run(['cc','-O2','-I'+str(source/'lib'),*values('C_DEFINES'),*values('C_INCLUDES'),'-c',str(root/'tests/airplay_direct.c'),'-o',str(work/'test.o')],check=True)
    subprocess.run(['c++',str(work/'test.o'),'-o',str(work/'test'),*libs],cwd=build,check=True)
    subprocess.run([str(work/'test')],check=True)
