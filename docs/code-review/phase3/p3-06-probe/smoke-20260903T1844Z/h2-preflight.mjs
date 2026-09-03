import h from "node:http2";
const [origin,path]=process.argv.slice(2);
const s=h.connect(`https://${origin}`);
const to=setTimeout(()=>{console.log(origin,path,"error TIMEOUT");s.destroy();process.exit(0)},20000);
s.on("connect",()=>{
  const alpn=s.socket.alpnProtocol;
  const r=s.request({":path":path});
  let n=0,cl,st;
  r.on("response",x=>{cl=x["content-length"];st=x[":status"]});
  r.on("data",c=>n+=c.length);
  r.on("end",()=>{clearTimeout(to);console.log(origin,path,"alpn",alpn,"status",st,"content-length",cl,"bytes",n);s.close()});
  r.on("error",e=>{clearTimeout(to);console.log(origin,path,"alpn",alpn,"stream-error",e.code||e.message);s.destroy()});
});
s.on("error",e=>{clearTimeout(to);console.log(origin,path,"error",e.code||e.message)});
