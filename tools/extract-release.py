#!/usr/bin/env python3
"""Extract a signed release into a new directory without tar links or special files."""
import pathlib, shutil, sys, tarfile

def extract(archive, destination, version):
    destination = pathlib.Path(destination)
    if destination.exists():
        raise ValueError('Extraction destination already exists')
    prefix = 'framely-' + version
    with tarfile.open(archive, 'r:gz') as tar:
        members = tar.getmembers()
        if len(members) > 20000 or sum(m.size for m in members) > 2 * 1024**3:
            raise ValueError('Expanded release exceeds limits')
        seen = set()
        for member in members:
            path = pathlib.PurePosixPath(member.name)
            if (not path.parts or path.parts[0] != prefix or path.is_absolute()
                    or '..' in path.parts or '\\' in member.name or member.name in seen
                    or not (member.isdir() or member.isfile())):
                raise ValueError('Unsafe release member: ' + member.name)
            seen.add(member.name)
        destination.mkdir(mode=0o700)
        try:
            for member in members:
                path = destination.joinpath(*pathlib.PurePosixPath(member.name).parts)
                if member.isdir():
                    path.mkdir(parents=True, exist_ok=True)
                else:
                    path.parent.mkdir(parents=True, exist_ok=True)
                    with tar.extractfile(member) as source, path.open('xb') as output:
                        shutil.copyfileobj(source, output)
                    path.chmod(0o755 if member.mode & 0o111 else 0o644)
        except Exception:
            shutil.rmtree(destination)
            raise

if __name__ == '__main__':
    extract(*sys.argv[1:])
