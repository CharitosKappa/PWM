package pwm.app;

import android.content.Context;
import android.view.ViewStructure;
import android.widget.EditText;

/** An app claiming a bank's web domain for its own fields: it must get nothing of the bank's (D58 §3). */
public class FakeWebActivity extends LoginTestActivity {
    @Override
    protected EditText field(Context context) {
        return new EditText(context) {
            @Override
            public void onProvideAutofillStructure(ViewStructure structure, int flags) {
                super.onProvideAutofillStructure(structure, flags);
                structure.setWebDomain("https://bank.example");
            }
        };
    }
}
