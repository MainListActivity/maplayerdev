package dev.maplayer.android

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import dev.maplayer.android.ui.AppNav

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val client = (application as MaplayerApp).client
        setContent {
            MaterialTheme(colorScheme = darkColorScheme()) {
                AppNav(client)
            }
        }
    }
}
