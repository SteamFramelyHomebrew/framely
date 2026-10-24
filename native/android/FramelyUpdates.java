// SPDX-License-Identifier: AGPL-3.0-only
// Android 11 helper. Runs only inside a Framely-launched container.
import android.app.*;
import android.content.*;
import android.content.pm.*;
import android.net.Uri;
import android.os.*;
import org.json.JSONObject;
import java.io.*;
import java.lang.reflect.*;
import java.util.*;
import java.util.concurrent.*;

public final class FramelyUpdates extends Binder {
 private static final String DESCRIPTOR="android.app.IActivityController";
 private static final long MAX_BYTES=8L*1024*1024*1024;
 private static final BlockingQueue<Runnable> work=new ArrayBlockingQueue<>(8);
 private static final Map<Integer,String> methods=new HashMap<>();
 private static final BlockingQueue<String> forwards=new ArrayBlockingQueue<>(64);
 private static final Set<String> allowed=new HashSet<>(),sessions=new HashSet<>();
 private static Context context;
 private static File directory;
 private static void log(String event){System.out.println("Framely APK updates: "+event);}
 private static void publish(JSONObject data, String id)throws Exception {
  File temporary=new File(directory,id+".json.part"), ready=new File(directory,id+".json");
  try(FileOutputStream f=new FileOutputStream(temporary)){f.write(data.toString().getBytes("UTF-8"));f.getFD().sync();}
  if(!temporary.renameTo(ready))throw new IOException("Cannot publish update request");
 }
 private static void failure(String reason){
  log(reason);
  try {String id=UUID.randomUUID().toString().replace("-","");publish(new JSONObject().put("error",reason),id);}catch(Exception ignored){}
 }
 public boolean onTransact(int code,Parcel data,Parcel reply,int flags)throws RemoteException {
  if(code==INTERFACE_TRANSACTION){reply.writeString(DESCRIPTOR);return true;}
  String method=methods.get(code);
  if(method==null)return super.onTransact(code,data,reply,flags);
  data.enforceInterface(DESCRIPTOR);
  boolean allow=true;
  if(method.equals("activityStarting")) {
   Intent i=data.readInt()!=0?Intent.CREATOR.createFromParcel(data):null;data.readString();
   String action=i==null?null:i.getAction();
   if(i!=null && (Intent.ACTION_INSTALL_PACKAGE.equals(action)||"application/vnd.android.package-archive".equals(i.getType()))) {
    final String uri=i.getDataString();allow=false;
    if(uri==null || !work.offer(()->capture(uri,false)))failure("Cannot queue APK installation request; retry from the application");
   } else if("android.content.pm.action.CONFIRM_INSTALL".equals(action)) {
    // ActivityController receives cloneFilter(), so Session ID extras are absent.
    if(hasManagedSession()){allow=false;if(!work.offer(FramelyUpdates::captureSessions))failure("Update queue is busy; retry from the application");}
   }
  }
  reply.writeNoException();
  if(method.equals("appNotResponding")||method.equals("appEarlyNotResponding")||method.equals("systemNotResponding"))reply.writeInt(method.equals("systemNotResponding")?-1:0);
  else reply.writeInt(allow?1:0);
  return true;
 }
 private static void forward(String text) {
  // Opening Steam's FIFO can block when Steam is stopped. Keep that off Binder/reader threads.
  if(!forwards.offer(text))log("Steam URI forwarding queue is full");
 }
 private static final class ProviderInput implements AutoCloseable {
  InputStream stream;Object manager;IBinder token;String authority;
  public void close()throws Exception {try{if(stream!=null)stream.close();}finally{if(manager!=null)Class.forName("android.app.IActivityManager").getMethod("removeContentProviderExternal",String.class,IBinder.class).invoke(manager,authority,token);}}
 }
 private static ProviderInput open(Uri uri)throws Exception {
  ProviderInput in=new ProviderInput();
  if("file".equals(uri.getScheme())) {
   File file=new File(uri.getPath()).getCanonicalFile();String path=file.getPath();
   List<String> bases=new ArrayList<>(Arrays.asList("/sdcard","/storage/emulated/0","/data/media/0"));
   for(String pkg:allowed){bases.add("/data/user/0/"+pkg);bases.add("/data/data/"+pkg);}
   boolean owned=false;
   for(String base:bases){File root=new File(base).getCanonicalFile();if(root.isDirectory() && !root.getPath().equals("/"))owned|=path.startsWith(root.getPath()+"/");}
   if(!owned || !file.isFile())throw new IOException("APK file is outside application storage");
   in.stream=new FileInputStream(file);return in;
  }
  if(!"content".equals(uri.getScheme()) || uri.getUserInfo()!=null)throw new IOException("Unsupported APK URI");
  ProviderInfo info=context.getPackageManager().resolveContentProvider(uri.getAuthority(),0);
  boolean downloads=info!=null && ("com.android.providers.downloads".equals(info.packageName)||"com.android.externalstorage".equals(info.packageName));
  if(info==null || !(allowed.contains(info.packageName)||downloads))throw new IOException("APK provider does not belong to a managed application");
  in.authority=uri.getAuthority();in.token=new Binder();
  in.manager=Class.forName("android.app.ActivityManager").getMethod("getService").invoke(null);
  try {
   Object holder=Class.forName("android.app.IActivityManager").getMethod("getContentProviderExternal",String.class,int.class,IBinder.class,String.class).invoke(in.manager,in.authority,0,in.token,"Framely APK update");
   Object provider=holder.getClass().getField("provider").get(holder);
   Method m=Class.forName("android.content.IContentProvider").getMethod("openFile",String.class,String.class,Uri.class,String.class,Class.forName("android.os.ICancellationSignal"),IBinder.class);
   ParcelFileDescriptor fd=(ParcelFileDescriptor)m.invoke(provider,null,null,uri,"r",null,null);
   in.stream=new ParcelFileDescriptor.AutoCloseInputStream(fd);return in;
  }catch(Exception e){in.close();throw e;}
 }
 private static boolean stage(InputStream in,String id)throws Exception {
  File partial=new File(directory,id+".apk.part"),apk=new File(directory,id+".apk");
  try {
   File[] pending=directory.listFiles(f->f.getName().endsWith(".apk")||f.getName().endsWith(".apk.part"));
   long retained=0;if(pending!=null)for(File f:pending)retained+=f.length();
   if(pending!=null && (pending.length>=8 || retained>=MAX_BYTES))throw new IOException("Update queue storage is full");
   try(FileOutputStream f=new FileOutputStream(partial)){
    byte[] bytes=new byte[262144];long total=0;int n;
    while((n=in.read(bytes))!=-1){total+=n;if(total+retained>MAX_BYTES||directory.getUsableSpace()<256*1024*1024)throw new IOException("Insufficient space for APK extraction");f.write(bytes,0,n);}f.getFD().sync();
   }
   PackageInfo info=context.getPackageManager().getPackageArchiveInfo(partial.getPath(),0);
   if(info==null || !allowed.contains(info.packageName))return false;
   PackageInfo installed=context.getPackageManager().getPackageInfo(info.packageName,0);
   if(info.getLongVersionCode()<installed.getLongVersionCode())throw new IOException("Application requested an APK downgrade");
   if(!partial.renameTo(apk))throw new IOException("Cannot save extracted APK");
   publish(new JSONObject().put("package",info.packageName).put("bytes",apk.length()),id);
   log("APK captured request="+id+" bytes="+apk.length());return true;
  }finally{partial.delete();}
 }
 private static void capture(String text,boolean view){
  String id=UUID.randomUUID().toString().replace("-","");
  try(ProviderInput in=open(Uri.parse(text))){
   if(!stage(in.stream,id)){if(view)forward("steam://openurl/"+text);else failure("APK is not an update to an installed application");}
  }catch(Exception e){if(view)forward("steam://openurl/"+text);else failure("Cannot extract application update; retry or select its APK in Framely");}
 }
 private static boolean hasManagedSession(){
  long identity=Binder.clearCallingIdentity();
  try {
   for(PackageInstaller.SessionInfo info:context.getPackageManager().getPackageInstaller().getAllSessions())
    if(info.isSealed() && info.isCommitted() && allowed.contains(info.getInstallerPackageName()))return true;
   return false;
  }catch(Exception e){failure("Cannot inspect installation session; original installer retained");return false;}
  finally{Binder.restoreCallingIdentity(identity);}
 }
 private static void captureSessions(){
  try {
   PackageInstaller installer=context.getPackageManager().getPackageInstaller();
   for(PackageInstaller.SessionInfo info:installer.getAllSessions()) {
    String id=Integer.toString(info.getSessionId());
    if(!info.isSealed()||!info.isCommitted()||!allowed.contains(info.getInstallerPackageName())||!sessions.add(id))continue;
    if(info.isMultiPackage()||info.isStaged()||info.hasParentSessionId()){failure("Multi-package and staged updates are not supported; select a complete APK in Framely");continue;}
    File stage=new File("/data/app/vmdl"+id+".tmp");File[] apks=stage.listFiles(f->f.isFile()&&f.getName().endsWith(".apk"));
    if(apks==null||apks.length!=1){failure("Split APK update cannot be installed as a single APK; select a complete APK in Framely");continue;}
    String key=UUID.randomUUID().toString().replace("-","");
    try(InputStream stream=new FileInputStream(apks[0])) {
     if(!stage(stream,key)){failure("Session APK does not match an installed application");continue;}
    }
    // Only abandon the original session after retaining the complete APK.
    installer.abandonSession(info.getSessionId());
   }
  }catch(Exception e){failure("Cannot extract pending installation session; retry from the application");}
 }
 public static void main(String[] args)throws Exception {
  if(args.length!=2)throw new IllegalArgumentException("Expected private update directory and package list");
  directory=new File(args[0]);
  try(BufferedReader r=new BufferedReader(new FileReader(args[1]))){String p;while((p=r.readLine())!=null)if(p.matches("[A-Za-z][A-Za-z0-9_]*(\\.[A-Za-z0-9_]+)+"))allowed.add(p);}
  Looper.prepareMainLooper();Class<?> thread=Class.forName("android.app.ActivityThread");Object at=thread.getMethod("systemMain").invoke(null);context=(Context)thread.getMethod("getSystemContext").invoke(at);
  Class<?> stub=Class.forName("android.app.IActivityController$Stub");
  for(Field f:stub.getDeclaredFields())if(f.getName().startsWith("TRANSACTION_")){f.setAccessible(true);methods.put(f.getInt(null),f.getName().substring(12));}
  FramelyUpdates gate=new FramelyUpdates();Class<?> iface=Class.forName("android.app.IActivityController");
  Object proxy=Proxy.newProxyInstance(iface.getClassLoader(),new Class[]{iface},(p,m,a)->m.getName().equals("asBinder")?gate:null);
  Object manager=Class.forName("android.app.ActivityManager").getMethod("getService").invoke(null);
  Class.forName("android.app.IActivityManager").getMethod("setActivityController",iface,boolean.class).invoke(manager,proxy,false);
  new Thread(()->{while(true)try{String line=forwards.take();try(FileOutputStream f=new FileOutputStream("/lepton/framely-original.pipe")){f.write((line+"\n").getBytes("UTF-8"));}}catch(Exception e){log("Steam URI forwarding failed");}},"steam-forward").start();
  new Thread(()->{while(true)try{work.take().run();}catch(Exception e){failure("Update extraction worker failed");}},"apk-extract").start();
  new Thread(()->{while(true)try(BufferedReader reader=new BufferedReader(new FileReader("/lepton/steam.pipe"))){
   String line;while((line=reader.readLine())!=null){
    if(line.length()>16384){log("Oversized URI ignored");continue;}
    if(line.startsWith("steam://openurl/content://")||line.startsWith("steam://openurl/file://")){
     final String uri=line.substring(16);if(!work.offer(()->capture(uri,true)))failure("Update extraction queue is full; retry from the application");
    }else forward(line);
   }
  }catch(Exception e){log("Steam URI pipe unavailable");try{Thread.sleep(1000);}catch(InterruptedException ignored){}}},"apk-uri").start();
  new File(directory,"ready").createNewFile();log("ready");
  Class.forName("com.android.internal.os.BinderInternal").getMethod("joinThreadPool").invoke(null);
 }
}
