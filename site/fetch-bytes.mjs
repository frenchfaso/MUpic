/** Fetch a boot asset while retaining useful context for network/body errors. */
export async function fetchBytes(url,onProgress=()=>{},{fetcher=globalThis.fetch.bind(globalThis),baseUrl=import.meta.url,cache='no-store'}={}) {
  const assetUrl=new URL(url,baseUrl).href;
  let stage='request',loaded=0;
  try {
    const response=await fetcher(assetUrl,{cache});
    if(!response.ok)throw new Error(`HTTP ${response.status}`);
    const total=Number(response.headers.get('Content-Length'))||0;
    stage='response body';
    if(!response.body) {
      const bytes=new Uint8Array(await response.arrayBuffer());loaded=bytes.length;
      onProgress(loaded,total||loaded);return bytes;
    }
    const reader=response.body.getReader(),chunks=[];
    let lastUpdate=-Infinity;
    while(true) {
      const {value,done}=await reader.read();if(done)break;
      chunks.push(value);loaded+=value.length;
      if(performance.now()-lastUpdate>=100){onProgress(loaded,total);lastUpdate=performance.now();}
    }
    const bytes=new Uint8Array(loaded);let offset=0;
    for(const chunk of chunks){bytes.set(chunk,offset);offset+=chunk.length;}
    onProgress(loaded,total||loaded);return bytes;
  } catch(error) {
    throw new Error(`Could not load ${assetUrl} (${stage}, ${loaded} bytes): ${error.message||error}`,{cause:error});
  }
}
