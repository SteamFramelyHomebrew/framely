// Render inside Steam's existing CEF context; no external runtime/fonts needed.
async function(p, selectPalette) {
    const load = data => new Promise((resolve,reject)=>{
        const image=new Image(), timer=setTimeout(()=>reject(Error('Artwork image timeout')),4000);
        image.onload=()=>{clearTimeout(timer);resolve(image);};
        image.onerror=()=>{clearTimeout(timer);reject(Error('Invalid artwork image'));};image.src=data;
    });
    const source=await load(p.image);
    const canvas=(w,h)=>{const c=document.createElement('canvas');c.width=w;c.height=h;return c;};
    const sample=canvas(64,64), sc=sample.getContext('2d');sc.drawImage(source,0,0,64,64);
    const palette=selectPalette(sc.getImageData(0,0,64,64).data,64,64);
    let n=2166136261;
    for(const c of p.package)n=Math.imul(n^c.charCodeAt(0),16777619);
    const random=()=>{n+=0x6D2B79F5;let t=n;t=Math.imul(t^(t>>>15),t|1);t^=t+Math.imul(t^(t>>>7),t|61);return ((t^(t>>>14))>>>0)/4294967296;};
    const angle=(-39-random()*8)*Math.PI/180, offset=(random()-.5)*65;
    const shade=(hex,f)=>'#'+[1,3,5].map(i=>Math.round(parseInt(hex.slice(i,i+2),16)*f).toString(16).padStart(2,'0')).join('');
    const rounded=(c,x,y,w,h,r)=>{c.beginPath();c.roundRect(x,y,w,h,r);};
    const icon=(c,x,y,s,rotation=0,opacity=1)=>{
        c.save();c.globalAlpha=opacity;c.translate(x+s/2,y+s/2);c.rotate(rotation*Math.PI/180);c.translate(-s/2,-s/2);
        rounded(c,0,0,s,s,s*.23);c.clip();
        const scale=Math.min(s/source.width,s/source.height),w=source.width*scale,h=source.height*scale;
        c.drawImage(source,(s-w)/2,(s-h)/2,w,h);c.restore();
    };
    const fontSize=(size,max)=>{
        // Use actual font metrics, including CJK/emoji and long custom names.
        const c=sc;c.font=`700 ${size}px "Noto Sans CJK SC", "Noto Sans", sans-serif`;
        const width=c.measureText(p.name).width;return width>max?size*max/width:size;
    };
    const title=(c,x,y,size,max,center=false)=>{
        c.save();c.fillStyle='#fff';c.textAlign=center?'center':'left';c.font=`700 ${fontSize(size,max)}px "Noto Sans CJK SC", "Noto Sans", sans-serif`;
        c.fillText(p.name,x,y);c.restore();
    };
    const base=(c,w,h)=>{
        const g=c.createLinearGradient(0,0,w,h);g.addColorStop(0,palette.base);g.addColorStop(.43,shade(palette.base,.7));g.addColorStop(1,shade(palette.accent,.24));
        c.fillStyle=g;c.fillRect(0,0,w,h);
        c.save();c.translate(w*.82,h*.82);c.scale(w*.7,h*.85);
        const light=c.createRadialGradient(0,0,0,0,0,1);light.addColorStop(0,palette.accent+'0f');light.addColorStop(1,palette.accent+'00');c.fillStyle=light;c.fillRect(-1,-1,2,2);c.restore();
    };
    const pill=(c,x,y,w,h,muted=false,opacity=1)=>{
        c.save();c.translate(x+w/2,y+h/2);c.rotate(angle);c.translate(-w/2,-h/2);
        c.globalAlpha=muted?opacity:1;
        if(muted){const g=c.createLinearGradient(0,0,w,h);g.addColorStop(0,shade(palette.accent,.22));g.addColorStop(1,shade(palette.accent,.4));c.fillStyle=g;}else c.fillStyle=palette.accent;
        rounded(c,0,0,w,h,h/2);c.fill();c.restore();
    };
    const line=(c,x,y,w,h)=>{c.fillStyle=palette.accent;rounded(c,x,y,w,h,h/2);c.fill();};
    const result=[];
    for(const [kind,w,h] of [[0,600,900],[1,3840,1240],[2,1280,360],[3,920,430],[4,256,256]]){
        const image=canvas(w,h),c=image.getContext('2d');
        if(kind===0){base(c,w,h);pill(c,-205+offset,30,540,116);pill(c,206,-90+offset,425,98,true,.42);pill(c,-132,684,310,100);icon(c,315,676,490,-16,.5);icon(c,155,146,290);title(c,300,551,65,490,true);line(c,265,592,70,7);}
        if(kind===3){base(c,w,h);pill(c,-100+offset,-24,405,80);pill(c,700,-30+offset,355,100,true,.35);pill(c,692,350,350,104);pill(c,570,410,286,49,true,.55);icon(c,76,117,196);title(c,323,243,62,490);line(c,326,282,62,6);}
        if(kind===1){base(c,w,h);pill(c,-520,-140,1410,240);pill(c,1630+offset*3,-40,1300,310,true,.43);pill(c,2040,940,1420,350);pill(c,1700,1150,1090,155,true,.5);pill(c,3220,140,960,240,true,.32);}
        if(kind===2){icon(c,16,24,312);title(c,382,232,136,866);}
        if(kind===4)icon(c,0,0,256);
        result.push({kind,data:image.toDataURL('image/png').split(',')[1],canvas:image});
    }
    return result;
}
