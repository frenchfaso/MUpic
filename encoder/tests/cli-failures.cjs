/* Existing files and source videos survive failed conversions; FFmpeg is reaped. */
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');
const assert = require('node:assert/strict');
const { spawnSync, execFileSync } = require('node:child_process');
const { createHash } = require('node:crypto');
const repo = path.resolve(__dirname, '../..');
const binary = process.argv[2] || path.join(repo, 'encoder/target/release/mupic-encoder');
const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'mupic-failures-'));
const hash = file => createHash('sha256').update(fs.readFileSync(file)).digest('hex');
try {
  const source = path.join(directory, 'source.mp4');
  const target = path.join(directory, 'existing.mup');
  execFileSync('ffmpeg', ['-v','error','-nostdin','-f','lavfi','-i','color=c=red:size=640x360:rate=30','-t','0.48','-c:v','libx264','-pix_fmt','yuv420p','-y',source]);
  fs.writeFileSync(target, Buffer.from('Previously generated output must survive.'));
  const before = hash(target), sourceBefore = hash(source);
  let result = spawnSync(binary, [source, '-o', target, '--gpu', 'off'], { encoding: 'utf8' });
  assert.equal(result.status, 1); assert.match(result.stderr, /Output already exists/); assert.equal(hash(target), before);
  result = spawnSync(binary, [source, '-o', source, '--force', '--gpu', 'off'], { encoding: 'utf8' });
  assert.equal(result.status, 1); assert.match(result.stderr, /must not overwrite the source/); assert.equal(hash(source), sourceBefore);
  if (process.platform !== 'win32') {
    const commands = path.join(directory, 'commands'); fs.mkdirSync(commands);
    const pidLog = path.join(directory, 'ffmpeg.pid');
    const wrapper = '#!/usr/bin/env python3\nimport os,sys\nif "rawvideo" not in sys.argv: raise SystemExit(90)\nwith open(' + JSON.stringify(pidLog) + ',"w") as f: f.write(str(os.getpid()))\nsys.stdout.buffer.write(bytes(100));sys.stdout.buffer.flush()\nraise SystemExit(23)\n';
    fs.writeFileSync(path.join(commands,'ffmpeg'),wrapper,{mode:0o755});
    result = spawnSync(binary, [source,'-o',target,'--force','--gpu','off','--threads','1'], { encoding:'utf8',env:{...process.env,PATH:commands+path.delimiter+process.env.PATH} });
    assert.equal(result.status,1); assert.match(result.stderr,/Truncated video/); assert.equal(hash(target),before);
    assert.ok(!fs.readdirSync(directory).some(f=>f.includes('.partial-')));
    const pid=Number(fs.readFileSync(pidLog,'utf8'));
    assert.throws(()=>process.kill(pid,0), error=>error.code==='ESRCH');
  }
  console.log(JSON.stringify({passed:true,existingOutputPreserved:true,sourcePreserved:true,partialCleaned:true,decoderReaped:process.platform!=='win32'}));
} finally { fs.rmSync(directory,{recursive:true,force:true}); }
