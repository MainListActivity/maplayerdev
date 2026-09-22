package dev.maplayer.android.ui

import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.rememberNavController
import dev.maplayer.android.net.IrohClient

object Routes {
    const val PAIR = "pair"
    const val SESSIONS = "sessions"
    const val TERMINAL = "terminal/{sessionId}"
    const val PROFILES = "profiles"
    fun terminal(sessionId: String) = "terminal/$sessionId"
}

@Composable
fun AppNav(client: IrohClient) {
    val nav = rememberNavController()
    val start = if (client.isPaired) Routes.SESSIONS else Routes.PAIR
    val state = remember { AppState(client) }

    NavHost(navController = nav, startDestination = start) {
        composable(Routes.PAIR) {
            PairingScreen(state) {
                nav.navigate(Routes.SESSIONS) { popUpTo(Routes.PAIR) { inclusive = true } }
            }
        }
        composable(Routes.SESSIONS) {
            SessionsScreen(
                state,
                onOpen = { nav.navigate(Routes.terminal(it)) },
                onProfiles = { nav.navigate(Routes.PROFILES) },
                onUnpair = {
                    state.client.clearPairing()
                    nav.navigate(Routes.PAIR) { popUpTo(Routes.SESSIONS) { inclusive = true } }
                },
            )
        }
        composable(Routes.TERMINAL) { entry ->
            val sessionId = entry.arguments?.getString("sessionId") ?: return@composable
            TerminalScreen(state, sessionId) { nav.popBackStack() }
        }
        composable(Routes.PROFILES) {
            ProfilesScreen(state) { nav.popBackStack() }
        }
    }
}
