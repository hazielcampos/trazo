package com.trazo.app;

import android.app.Activity;
import android.app.Fragment;
import android.content.ClipData;
import android.content.ContentResolver;
import android.content.Intent;
import android.database.Cursor;
import android.net.Uri;
import android.os.Bundle;
import android.provider.OpenableColumns;

import java.io.File;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.io.OutputStream;
import java.util.ArrayList;

/**
 * Opens Android's own file picker (previews, folders, Drive, Downloads…)
 * and hands the chosen files to Trazo as local copies.
 *
 * NativeActivity doesn't pass activity results to native code, so a headless
 * fragment starts the picker and receives the result. Trazo loads this class
 * from bytes at runtime (see src/android.rs) and registers {@link #picked}.
 * Rebuild the dex with scripts/build-picker-dex.sh after editing.
 */
public class TrazoPicker extends Fragment {
    /** Implemented in Rust: the local paths of the chosen files (empty if cancelled). */
    public static native void picked(String[] paths);

    public static void open(final Activity activity, final String[] mimes, final boolean multiple) {
        activity.runOnUiThread(new Runnable() {
            public void run() {
                TrazoPicker f = new TrazoPicker();
                Bundle b = new Bundle();
                b.putStringArray("mimes", mimes);
                b.putBoolean("multiple", multiple);
                f.setArguments(b);
                activity.getFragmentManager().beginTransaction().add(f, "trazo_picker").commitAllowingStateLoss();
            }
        });
    }

    @Override
    public void onCreate(Bundle saved) {
        super.onCreate(saved);
        if (saved != null) {
            return;
        }
        String[] mimes = getArguments().getStringArray("mimes");
        Intent i = new Intent(Intent.ACTION_OPEN_DOCUMENT);
        i.addCategory(Intent.CATEGORY_OPENABLE);
        i.setType(mimes.length == 1 ? mimes[0] : "*/*");
        if (mimes.length > 1) {
            i.putExtra(Intent.EXTRA_MIME_TYPES, mimes);
        }
        i.putExtra(Intent.EXTRA_ALLOW_MULTIPLE, getArguments().getBoolean("multiple"));
        try {
            startActivityForResult(i, 7);
        } catch (Exception e) {
            finish();
            picked(new String[0]);
        }
    }

    @Override
    public void onActivityResult(int request, int result, Intent data) {
        final ArrayList<Uri> uris = new ArrayList<Uri>();
        if (result == Activity.RESULT_OK && data != null) {
            ClipData clip = data.getClipData();
            if (clip != null) {
                for (int k = 0; k < clip.getItemCount(); k++) {
                    uris.add(clip.getItemAt(k).getUri());
                }
            } else if (data.getData() != null) {
                uris.add(data.getData());
            }
        }
        final Activity activity = getActivity();
        finish();
        if (activity == null) {
            picked(new String[0]);
            return;
        }
        final ContentResolver cr = activity.getContentResolver();
        final File dir = new File(activity.getCacheDir(), "elegidos");
        // Copy off the UI thread (videos can be big).
        new Thread(new Runnable() {
            public void run() {
                dir.mkdirs();
                cleanOld(dir);
                ArrayList<String> out = new ArrayList<String>();
                for (Uri u : uris) {
                    try {
                        File f = unique(dir, displayName(cr, u));
                        InputStream in = cr.openInputStream(u);
                        OutputStream os = new FileOutputStream(f);
                        byte[] buf = new byte[1 << 16];
                        int n;
                        while ((n = in.read(buf)) > 0) {
                            os.write(buf, 0, n);
                        }
                        os.close();
                        in.close();
                        out.add(f.getAbsolutePath());
                    } catch (Exception e) {
                        // Skip what can't be read.
                    }
                }
                picked(out.toArray(new String[0]));
            }
        }).start();
    }

    private void finish() {
        try {
            getFragmentManager().beginTransaction().remove(this).commitAllowingStateLoss();
        } catch (Exception e) {
            // Already gone.
        }
    }

    private static String displayName(ContentResolver cr, Uri u) {
        String name = null;
        Cursor c = null;
        try {
            c = cr.query(u, new String[] { OpenableColumns.DISPLAY_NAME }, null, null, null);
            if (c != null && c.moveToFirst()) {
                name = c.getString(0);
            }
        } catch (Exception e) {
            // Fall back below.
        } finally {
            if (c != null) {
                c.close();
            }
        }
        if (name == null || name.isEmpty()) {
            name = u.getLastPathSegment();
        }
        if (name == null || name.isEmpty()) {
            name = "archivo";
        }
        return name.replace('/', '_');
    }

    private static File unique(File dir, String name) {
        File f = new File(dir, name);
        int dot = name.lastIndexOf('.');
        String base = dot > 0 ? name.substring(0, dot) : name;
        String ext = dot > 0 ? name.substring(dot) : "";
        for (int k = 2; f.exists(); k++) {
            f = new File(dir, base + " (" + k + ")" + ext);
        }
        return f;
    }

    /** Copies from earlier picks: dropped after a day (imports keep their own data). */
    private static void cleanOld(File dir) {
        File[] files = dir.listFiles();
        if (files == null) {
            return;
        }
        long cutoff = System.currentTimeMillis() - 24L * 3600L * 1000L;
        for (File f : files) {
            if (f.lastModified() < cutoff) {
                f.delete();
            }
        }
    }
}
