package pwm.app;

import android.app.Activity;
import android.content.Context;
import android.os.Bundle;
import android.text.InputType;
import android.view.View;
import android.widget.EditText;
import android.widget.LinearLayout;

/**
 * Another app's login screen, as the autofill service sees it (D58 §9). It
 * runs in the test app's own process, which has no Kotlin library, so it is
 * Java.
 */
public class LoginTestActivity extends Activity {
    protected EditText field(Context context) {
        return new EditText(context);
    }

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        EditText username = field(this);
        username.setContentDescription("username");
        username.setAutofillHints(View.AUTOFILL_HINT_USERNAME);
        username.setInputType(InputType.TYPE_CLASS_TEXT);
        // No keyboard on screen, so the dropdown shows.
        username.setShowSoftInputOnFocus(false);
        EditText password = field(this);
        password.setContentDescription("password");
        password.setAutofillHints(View.AUTOFILL_HINT_PASSWORD);
        password.setInputType(InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_PASSWORD);
        password.setShowSoftInputOnFocus(false);
        LinearLayout layout = new LinearLayout(this);
        layout.setOrientation(LinearLayout.VERTICAL);
        layout.setPadding(48, 200, 48, 48);
        layout.addView(username);
        layout.addView(password);
        setContentView(layout);
    }
}
