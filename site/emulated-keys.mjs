/** Preserve short host taps across CPU batches and network stalls. */
export class EmulatedKeys {
  constructor(apply) { this.apply=apply;this.keys=new Map(); }
  set(index,down,now) {
    let key=this.keys.get(index);
    if(!key){key={down:false,desired:false,at:-Infinity,queue:[]};this.keys.set(index,key);}
    down=Boolean(down);
    if(down===key.desired)return;
    // Bound queued repeats while always accepting the release of an accepted press.
    if(down&&key.queue.length>=128)return;
    key.desired=down;
    const previous=key.queue.at(-1)||key;
    const at=Math.max(now,previous.at+(previous.down?60:20));
    key.queue.push({down,at});this.advance(now);
  }
  advance(now) {
    for(const [index,key] of this.keys) {
      while(key.queue.length&&key.queue[0].at<=now) {
        const event=key.queue.shift();key.down=event.down;key.at=now;
        this.apply(index,event.down);
        // Measure the next hold from when it actually reached the emulated matrix.
        if(key.queue.length)key.queue[0].at=Math.max(key.queue[0].at,now+(key.down?60:20));
      }
    }
  }
  reset() {
    for(const [index,key] of this.keys)if(key.down)this.apply(index,false);
    this.keys.clear();
  }
}
