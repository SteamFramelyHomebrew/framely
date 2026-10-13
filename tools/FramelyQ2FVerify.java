// SPDX-License-Identifier: AGPL-3.0-only
import com.android.apksig.ApkVerifier;
import java.io.File;

/** Verify an OVRPort-produced APK before handing it to Framely's install review. */
public final class FramelyQ2FVerify {
    public static void main(String[] args) throws Exception {
        if (args.length != 1) {
            throw new IllegalArgumentException("Expected one APK path");
        }
        ApkVerifier.Result result = new ApkVerifier.Builder(new File(args[0])).build().verify();
        if (!result.isVerified()) {
            for (ApkVerifier.IssueWithParams issue : result.getErrors()) {
                System.err.println(issue);
            }
            throw new IllegalStateException("Quest converter produced an invalid APK signature");
        }
    }
}
