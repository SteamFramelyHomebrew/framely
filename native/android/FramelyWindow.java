import java.lang.reflect.*;
import java.util.List;
public final class FramelyWindow {
 public static void main(String[] args) throws Exception {
  if(args.length!=1 || !args[0].matches("[A-Za-z][A-Za-z0-9_]*(\\.[A-Za-z0-9_]+)+")) throw new IllegalArgumentException("Expected package name");
  Class<?> manager=Class.forName("android.app.ActivityTaskManager");
  Object service=manager.getDeclaredMethod("getService").invoke(null);
  Class<?> api=Class.forName("android.app.IActivityTaskManager");
  Method tasks=api.getMethod("getTasks",int.class);
  Method mode=api.getMethod("setTaskWindowingMode",int.class,int.class,boolean.class);
  for(int attempt=0;attempt<20;attempt++) {
   for(Object task:(List<?>)tasks.invoke(service,32)) {
    Object top=task.getClass().getField("topActivity").get(task);
    if(top==null || !args[0].equals(top.getClass().getMethod("getPackageName").invoke(top)))continue;
    int id=task.getClass().getField("taskId").getInt(task);
    if(Boolean.TRUE.equals(mode.invoke(service,id,1,true))) {System.out.println("FRAMELY_FULLSCREEN_OK");return;}
   }
   Thread.sleep(250);
  }
  throw new IllegalStateException("Target task could not enter fullscreen");
 }
}
