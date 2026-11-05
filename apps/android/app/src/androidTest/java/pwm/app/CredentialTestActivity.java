package pwm.app;

import android.app.Activity;
import android.credentials.CreateCredentialException;
import android.credentials.CreateCredentialRequest;
import android.credentials.CreateCredentialResponse;
import android.credentials.CredentialManager;
import android.credentials.CredentialOption;
import android.credentials.GetCredentialException;
import android.credentials.GetCredentialRequest;
import android.credentials.GetCredentialResponse;
import android.os.Bundle;
import android.os.CancellationSignal;
import android.os.OutcomeReceiver;
import android.widget.TextView;
import java.util.ArrayList;

/**
 * Another app asking the Credential Manager (D61 §10): it creates or uses a
 * passkey or a password through the system's sheet, then shows the result
 * for the test to read ("ok:" and the JSON, or "error:" and the type). It runs
 * in the test app's own process, which has neither Kotlin nor AndroidX, so it
 * is Java on the platform's API, with AndroidX's bundle keys.
 */
public class CredentialTestActivity extends Activity {
    public static final String ACTION = "action";
    public static final String JSON = "json";
    public static final String USER = "user";
    public static final String PASSWORD = "password";

    private static final String PUBLIC_KEY = "androidx.credentials.TYPE_PUBLIC_KEY_CREDENTIAL";
    private static final String PASSWORD_TYPE = "android.credentials.TYPE_PASSWORD_CREDENTIAL";
    private static final String SUBTYPE = "androidx.credentials.BUNDLE_KEY_SUBTYPE";
    private static final String REQUEST_JSON = "androidx.credentials.BUNDLE_KEY_REQUEST_JSON";
    private static final String DISPLAY_INFO = "androidx.credentials.BUNDLE_KEY_REQUEST_DISPLAY_INFO";
    private static final String USER_ID = "androidx.credentials.BUNDLE_KEY_USER_ID";
    private static final String ID = "androidx.credentials.BUNDLE_KEY_ID";
    private static final String SECRET = "androidx.credentials.BUNDLE_KEY_PASSWORD";

    private TextView result;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        result = new TextView(this);
        result.setContentDescription("result");
        result.setPadding(48, 200, 48, 48);
        result.setText("waiting");
        setContentView(result);
        String action = getIntent().getStringExtra(ACTION);
        CredentialManager manager = getSystemService(CredentialManager.class);
        if ("createPasskey".equals(action)) {
            Bundle data = new Bundle();
            data.putString(SUBTYPE, "androidx.credentials.BUNDLE_VALUE_SUBTYPE_CREATE_PUBLIC_KEY_CREDENTIAL_REQUEST");
            data.putString(REQUEST_JSON, getIntent().getStringExtra(JSON));
            data.putBundle(DISPLAY_INFO, display(getIntent().getStringExtra(USER)));
            create(manager, new CreateCredentialRequest.Builder(PUBLIC_KEY, data, data).build(),
                    "androidx.credentials.BUNDLE_KEY_REGISTRATION_RESPONSE_JSON");
        } else if ("getPasskey".equals(action)) {
            Bundle data = new Bundle();
            data.putString(SUBTYPE, "androidx.credentials.BUNDLE_VALUE_SUBTYPE_GET_PUBLIC_KEY_CREDENTIAL_OPTION");
            data.putString(REQUEST_JSON, getIntent().getStringExtra(JSON));
            get(manager, new CredentialOption.Builder(PUBLIC_KEY, data, data).build(),
                    "androidx.credentials.BUNDLE_KEY_AUTHENTICATION_RESPONSE_JSON");
        } else if ("createPassword".equals(action)) {
            String user = getIntent().getStringExtra(USER);
            Bundle data = new Bundle();
            data.putString(ID, user);
            data.putString(SECRET, getIntent().getStringExtra(PASSWORD));
            data.putBundle(DISPLAY_INFO, display(user));
            Bundle query = new Bundle();
            query.putBundle(DISPLAY_INFO, display(user));
            create(manager, new CreateCredentialRequest.Builder(PASSWORD_TYPE, data, query).build(), null);
        } else if ("getPassword".equals(action)) {
            Bundle data = new Bundle();
            data.putStringArrayList("androidx.credentials.BUNDLE_KEY_ALLOWED_USER_IDS", new ArrayList<>());
            get(manager, new CredentialOption.Builder(PASSWORD_TYPE, data, data).build(), null);
        }
    }

    private static Bundle display(String user) {
        Bundle display = new Bundle();
        display.putCharSequence(USER_ID, user);
        return display;
    }

    private void show(String text) {
        runOnUiThread(() -> result.setText(text));
    }

    private void create(CredentialManager manager, CreateCredentialRequest request, String key) {
        manager.createCredential(this, request, new CancellationSignal(), getMainExecutor(),
                new OutcomeReceiver<CreateCredentialResponse, CreateCredentialException>() {
                    @Override
                    public void onResult(CreateCredentialResponse response) {
                        show("ok:" + (key == null ? "saved" : response.getData().getString(key)));
                    }

                    @Override
                    public void onError(CreateCredentialException error) {
                        show("error:" + error.getType());
                    }
                });
    }

    private void get(CredentialManager manager, CredentialOption option, String key) {
        GetCredentialRequest request = new GetCredentialRequest.Builder(new Bundle()).addCredentialOption(option).build();
        manager.getCredential(this, request, new CancellationSignal(), getMainExecutor(),
                new OutcomeReceiver<GetCredentialResponse, GetCredentialException>() {
                    @Override
                    public void onResult(GetCredentialResponse response) {
                        Bundle data = response.getCredential().getData();
                        show("ok:" + (key == null ? data.getString(ID) + "|" + data.getString(SECRET) : data.getString(key)));
                    }

                    @Override
                    public void onError(GetCredentialException error) {
                        show("error:" + error.getType());
                    }
                });
    }
}
