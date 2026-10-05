export type SearchIndex={terms:string[]};
export function normalizeSearch(value:string){return value.normalize('NFKC').normalize('NFD').replace(/\p{M}/gu,mark=>mark==='\u3099'||mark==='\u309a'?mark:'').normalize('NFC').toLowerCase().replace(/[\s\p{P}\p{S}]/gu,'');}
export function matchesSearch(name:string,query:string,index?:SearchIndex){const needle=normalizeSearch(query);return !needle||[normalizeSearch(name),...(index?.terms??[])].some(term=>term.includes(needle));}
