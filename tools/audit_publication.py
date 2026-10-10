#!/usr/bin/env python3
"""Audit the exact public Git payload, including the two authorized LFS movies."""
import argparse, hashlib, json, re, subprocess
from pathlib import Path
ROOT=Path(__file__).resolve().parent.parent
PREFIXES={'assets','demo','docs','encoder','media','player','releases','site','tests','tools','.github','reports'}
ROOT_FILES={'.gitignore','.gitattributes','README.md','CREDITS.md','LICENSE'}
FORBIDDEN={'target','cache','__pycache__','node_modules','.env','.DS_Store','usb-work'}
SECRETS=[re.compile(rb'-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----'),
         re.compile(rb'(?:ghp_|gho_|ghu_|ghs_|ghr_)[A-Za-z0-9]{30,}'),
         re.compile(rb'github_pat_[A-Za-z0-9_]{50,}'),re.compile(rb'AKIA[A-Z0-9]{16}')]

def run(*args):return subprocess.check_output(args,cwd=ROOT)
def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',type=Path,default=ROOT/'build'/'publication-audit.json')
    args=parser.parse_args()
    names=sorted(set(run('git','ls-files','-z','--cached','--others','--exclude-standard').decode().split('\0'))-{''})
    specs=json.loads((ROOT/'media'/'checksums.json').read_text())['files']
    entries=[];git_bytes=0;lfs_bytes=0
    for name in names:
        p=Path(name);f=ROOT/p
        if p.parts[0] not in PREFIXES and name not in ROOT_FILES:raise ValueError(f'Not in public allowlist: {name}')
        if any(part in FORBIDDEN for part in p.parts) or f.is_symlink() or not f.is_file():raise ValueError(f'Forbidden payload: {name}')
        if name.startswith('site/media/'):raise ValueError('Generated streaming blocks belong only in the Pages artifact')
        data=f.read_bytes();sha=hashlib.sha256(data).hexdigest();mode='git'
        if p.suffix.lower() in {'.mup','.img'}:raise ValueError(f'Uncompressed media must never be published: {name}')
        if name.startswith('media/') and p.name.endswith('.MUP.gz'):
            if p.parent!=Path('media') or p.name[:-3] not in specs:raise ValueError(f'Unapproved media: {name}')
            spec=specs[p.name[:-3]]
            if len(data)!=spec['compressed_bytes'] or sha!=spec['compressed_sha256']:raise ValueError(f'Media checksum mismatch: {name}')
            attributes=run('git','check-attr','filter','--',name).decode()
            if not attributes.strip().endswith(': lfs'):raise ValueError(f'Large media is not LFS tracked: {name}')
            mode='lfs';lfs_bytes+=len(data)
        else:
            if len(data)>=100_000_000:raise ValueError(f'Git blob exceeds 100 MB: {name}')
            if b'\0' not in data and any(pattern.search(data) for pattern in SECRETS):raise ValueError(f'Potential secret in public file: {name}')
            git_bytes+=len(data)
        entries.append({'path':name,'bytes':len(data),'sha256':sha,'storage':mode})
    flash=(ROOT/'site/data/flash.bin').read_bytes()
    if len(flash)!=16<<20 or flash[0xfe8000:]!=b'\xff'*0x18000:raise ValueError('Private flash configuration pages were not erased')
    for name in ['LICENSE','CREDITS.md','demo/emulator/LICENSE','demo/emulator/vendor/trx64-core/LICENSE','demo/emulator/vendor/resid-rs/LICENSE']:
        if not (ROOT/name).exists():raise ValueError(f'Missing third-party license: {name}')
    result={'status':'passed','repository':'frenchfaso/MUpic','public_git_files':len(entries),
            'git_payload_bytes':git_bytes,'authorized_lfs_payload_bytes':lfs_bytes,
            'flash_configuration_pages':'erased','secret_scan':'passed','files':entries}
    args.output.parent.mkdir(parents=True,exist_ok=True)
    args.output.write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps({k:v for k,v in result.items() if k!='files'},indent=2))
if __name__=='__main__':main()
