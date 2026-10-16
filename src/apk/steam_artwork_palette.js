// Pure image algorithm. Package IDs and application names never enter selection.
function(pixels, width, height) {
    if (pixels.length !== width * height * 4) throw Error('Invalid RGBA image');
    const distance = (a, b) => a.reduce((sum, v, i) => sum + (v - b[i]) ** 2, 0);
    const average = (points, key) => {
        const weight = points.reduce((sum, p) => sum + p.weight, 0);
        return [0, 1, 2].map(i => points.reduce((sum, p) => sum + p[key][i] * p.weight, 0) / weight);
    };
    const hex = rgb => '#' + rgb.map(v => Math.max(0, Math.min(255, Math.round(v))).toString(16).padStart(2, '0')).join('');
    const lab = rgb => {
        const [r, g, b] = rgb.map(v => { v /= 255; return v <= .04045 ? v / 12.92 : ((v + .055) / 1.055) ** 2.4; });
        const l = Math.cbrt(.4122214708*r + .5363325363*g + .0514459929*b);
        const m = Math.cbrt(.2119034982*r + .6806995451*g + .1073969566*b);
        const s = Math.cbrt(.0883024619*r + .2817188376*g + .6299787005*b);
        return [.2104542553*l + .793617785*m - .0040720468*s, 1.9779984951*l - 2.428592205*m + .4505937099*s, .0259040371*l + .7827717662*m - .808675766*s];
    };
    const hsl = rgb => {
        const [r,g,b] = rgb.map(v => v/255), hi = Math.max(r,g,b), lo = Math.min(r,g,b), d = hi-lo, l = (hi+lo)/2;
        const h = d === 0 ? 0 : hi === r ? (g-b)/d : hi === g ? (b-r)/d+2 : (r-g)/d+4;
        return [(h*60+360)%360, d === 0 ? 0 : d/(1-Math.abs(2*l-1)), l];
    };
    const fromHsl = (h,s,l) => {
        const a=s*Math.min(l,1-l), f=n=>{const k=(n+h/30)%12;return Math.round(255*(l-a*Math.max(-1,Math.min(k-3,9-k,1))));};
        return [f(0),f(8),f(4)];
    };
    const points = [];
    for (let y=0; y<height; y++) for (let x=0; x<width; x++) {
        const i=(y*width+x)*4; if(pixels[i+3]<32) continue;
        const rgb=Array.from(pixels.subarray(i,i+3));
        points.push({rgb,lab:lab(rgb),weight:pixels[i+3]/255,edge:x<width*.19||x>=width*.81||y<height*.19||y>=height*.81});
    }
    const neutral = {selected:'#808080',base:'#191a1d',accent:'#9a9da5',neutral:true};
    if(!points.length) return neutral;
    let centers=[average(points,'lab')], groups;
    while(centers.length<8) {
        const nearest=p=>Math.min(...centers.map(c=>distance(p.lab,c)));
        const far=points.reduce((best,p)=>nearest(p)>nearest(best)?p:best,points[0]);
        if(nearest(far)<.0004) break;
        centers.push(far.lab);
    }
    for(let step=0;step<24;step++) {
        groups=centers.map(()=>[]);
        for(const p of points) {const ds=centers.map(c=>distance(p.lab,c));groups[ds.indexOf(Math.min(...ds))].push(p);}
        const next=groups.map((g,i)=>g.length?average(g,'lab'):centers[i]);
        const delta=Math.max(...next.map((c,i)=>distance(c,centers[i])));
        centers=next;if(delta<.0000001)break;
    }
    const clusters=groups.filter(g=>g.length).map(points=>({points,lab:average(points,'lab')}));
    for(let changed=true;changed;) {
        changed=false;
        outer:for(let i=0;i<clusters.length;i++)for(let j=i+1;j<clusters.length;j++)if(distance(clusters[i].lab,clusters[j].lab)<.065**2) {
            clusters[i].points.push(...clusters[j].points);clusters[i].lab=average(clusters[i].points,'lab');clusters.splice(j,1);changed=true;break outer;
        }
    }
    const total=points.reduce((s,p)=>s+p.weight,0), edgeTotal=points.filter(p=>p.edge).reduce((s,p)=>s+p.weight,0);
    const candidates=clusters.map(c=>{
        const coverage=c.points.reduce((s,p)=>s+p.weight,0)/total;
        const edgeCoverage=edgeTotal?c.points.filter(p=>p.edge).reduce((s,p)=>s+p.weight,0)/edgeTotal:coverage;
        const rgb=average(c.points,'rgb'), chroma=Math.hypot(c.lab[1],c.lab[2]);
        const eligible=coverage>=.035&&edgeCoverage>=.025&&chroma>=.012&&c.lab[0]>.18&&c.lab[0]<.96;
        return {rgb,coverage,edgeCoverage,eligible,score:(.5*coverage+.5*edgeCoverage)*Math.min(1,chroma/.025)*(chroma>.16?.8:1)};
    }).sort((a,b)=>b.score-a.score);
    const best=candidates.find(c=>c.eligible);
    if(!best||best.score<.055)return {...neutral,selected:hex([...candidates].sort((a,b)=>(b.coverage+b.edgeCoverage)-(a.coverage+a.edgeCoverage))[0].rgb)};
    const [h,s]=hsl(best.rgb);
    // Multiply all RGB channels uniformly: 80% intensity, no hue remapping.
    return {selected:hex(best.rgb),base:hex(fromHsl(h,s*.22,.095)),accent:hex(best.rgb.map(v=>v*.8)),neutral:false};
}
