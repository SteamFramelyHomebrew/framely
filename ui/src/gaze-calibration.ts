export type GazePoint={x:number;y:number};
export type GazeCalibration={space:'headAngles';matrix:number[];width:number;height:number;error:number;maxError:number};
export type CalibrationPair={raw:GazePoint;target:GazePoint;group?:number};
export function correctedGaze(p:GazePoint,c?:GazeCalibration|null):GazePoint{if(!c)return p;const m=c.matrix;return {x:m[0]*p.x+m[1]*p.y+m[2],y:m[3]*p.x+m[4]*p.y+m[5]};}
export function angularError(a:GazePoint,b:GazePoint){const dot=Math.sin(a.y)*Math.sin(b.y)+Math.cos(a.y)*Math.cos(b.y)*Math.cos(a.x-b.x);return Math.acos(Math.max(-1,Math.min(1,dot)));}
export function median(values:number[]){const v=[...values].sort((a,b)=>a-b);return v.length%2?v[(v.length-1)/2]:(v[v.length/2-1]+v[v.length/2])/2;}
export function fixation(samples:GazePoint[]){const center={x:median(samples.map(p=>p.x)),y:median(samples.map(p=>p.y))};const spread=median(samples.map(p=>Math.hypot(p.x-center.x,p.y-center.y)));return {center,spread};}
// Average paired directions over short time slices, never raw gaze alone.
// Head movement remains in both sides of each pair. This reduces the noisy
// regressor bias of fitting gains against every individual eye camera frame.
export function calibrationBlocks(samples:CalibrationPair[],group:number,size=6):CalibrationPair[]{
 const blocks:CalibrationPair[]=[];for(let i=0;i+size<=samples.length;i+=size){const part=samples.slice(i,i+size);const average=(field:'raw'|'target')=>({x:part.reduce((s,p)=>s+p[field].x,0)/size,y:part.reduce((s,p)=>s+p[field].y,0)/size});blocks.push({raw:average('raw'),target:average('target'),group});}return blocks;
}
export function calibrationValidation(samples:CalibrationPair[],model:GazeCalibration){
 const residuals=samples.map(p=>{const q=correctedGaze(p.raw,model);return {x:q.x-p.target.x,y:q.y-p.target.y};});
 const center={x:median(residuals.map(p=>p.x)),y:median(residuals.map(p=>p.y))};
 // Measure fixation bias in each frame's coordinates, then take its median.
 // A world/window median would mix together different head poses.
 const bias=median(samples.map(p=>angularError({x:p.target.x+center.x,y:p.target.y+center.y},p.target)));
 const errors=samples.map(p=>angularError(correctedGaze(p.raw,model),p.target)).sort((a,b)=>a-b);
 const baseline=median(samples.map(p=>angularError(p.raw,p.target)));
 return {bias,baseline,p95:errors[Math.min(errors.length-1,Math.floor(errors.length*.95))],jitter:fixation(residuals).spread};
}
export function fitCalibration(pairs:CalibrationPair[],width:number,height:number):GazeCalibration|null{
 if(pairs.length<6)return null;
 const counts=new Map<number,number>();for(const p of pairs)counts.set(p.group??0,(counts.get(p.group??0)??0)+1);
 const base=pairs.map(p=>1/counts.get(p.group??0)!);let weights=[...base],matrix:number[]=[];
 const solve=(axis:'x'|'y')=>{const a=Array.from({length:3},()=>[0,0,0,0]);pairs.forEach((p,n)=>{const row=[p.raw.x,p.raw.y,1],w=weights[n];for(let i=0;i<3;i++){for(let j=0;j<3;j++)a[i][j]+=w*row[i]*row[j];a[i][3]+=w*row[i]*p.target[axis];}});
 const prior=weights.reduce((s,w)=>s+w,0)*.0005;a[0][0]+=prior;a[1][1]+=prior;a[axis==='x'?0:1][3]+=prior;
 for(let i=0;i<3;i++){let pivot=i;for(let j=i+1;j<3;j++)if(Math.abs(a[j][i])>Math.abs(a[pivot][i]))pivot=j;[a[i],a[pivot]]=[a[pivot],a[i]];if(Math.abs(a[i][i])<1e-7)return null;const divisor=a[i][i];for(let k=i;k<4;k++)a[i][k]/=divisor;for(let j=0;j<3;j++)if(j!==i){const factor=a[j][i];for(let k=i;k<4;k++)a[j][k]-=factor*a[i][k];}}return a.map(row=>row[3]);};
 // Huber reweighting stops a bad fixation from steering the whole fit.
 for(let iteration=0;iteration<6;iteration++){const x=solve('x'),y=solve('y');if(!x||!y)return null;matrix=[...x,...y];const candidate={space:'headAngles' as const,matrix,width,height,error:0,maxError:0};const errors=pairs.map(p=>angularError(correctedGaze(p.raw,candidate),p.target));const cutoff=Math.max(.008,median(errors)*2);weights=errors.map((e,i)=>base[i]*Math.min(1,cutoff/Math.max(e,1e-9)));}
 const det=matrix[0]*matrix[4]-matrix[1]*matrix[3];if(matrix.some(v=>!Number.isFinite(v)||Math.abs(v)>4)||det<.25||det>4||Math.abs(matrix[2])>.5||Math.abs(matrix[5])>.5)return null;
 const result:GazeCalibration={space:'headAngles',matrix,width,height,error:0,maxError:0};const errors=pairs.map(p=>angularError(correctedGaze(p.raw,result),p.target));result.error=Math.sqrt(errors.reduce((sum,e)=>sum+e*e,0)/errors.length);result.maxError=Math.max(...errors);return result;
}
