package net.haven.app

import android.app.UiModeManager
import android.content.res.Configuration
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/** Haven's one window. Section K0: shows that the app starts and libmpv
 *  loads; the real shell comes in K9. */
class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        val tv = (getSystemService(UI_MODE_SERVICE) as UiModeManager).currentModeType ==
            Configuration.UI_MODE_TYPE_TELEVISION
        val mpv = Mpv.apiVersion()
        setContent { Starting(tv = tv, mpv = mpv) }
    }
}

@Composable
private fun Starting(tv: Boolean, mpv: Long?) {
    Column(
        modifier = Modifier.fillMaxSize().background(Color(0xFF0D0F14)),
        verticalArrangement = Arrangement.Center,
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Image(painterResource(R.drawable.banner), contentDescription = null, modifier = Modifier.size(width = 320.dp, height = 180.dp))
        Spacer(Modifier.height(16.dp))
        Text("Haven", color = Color.White, fontSize = 32.sp, fontWeight = FontWeight.Bold)
        Text(
            "${BuildConfig.VERSION_NAME} build ${BuildConfig.HAVEN_BUILD} · Kotlin" +
                (if (tv) " · Android TV" else "") +
                (mpv?.let { " · libmpv ${it shr 16}.${it and 0xffff}" } ?: " · libmpv missing"),
            color = Color(0xFFE8B94A),
            fontSize = 14.sp,
        )
    }
}
