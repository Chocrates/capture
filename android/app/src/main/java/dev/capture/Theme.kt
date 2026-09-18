package dev.capture

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color

/**
 * One palette, two grounds.
 *
 * The application had no theme at all — `MaterialTheme { }` with no arguments,
 * which is Material's light baseline whatever the device is set to. So a phone
 * in dark mode opened a white sheet, at night, from a lock screen. §2.1 says
 * capture has to cost almost nothing at the moment a thought arrives; being
 * dazzled is a cost, and it is charged exactly when somebody reaches for the
 * thing in the dark.
 *
 * # Following the system rather than offering a switch
 *
 * A theme toggle is a preference, and a preference is a thing to store, sync,
 * reconcile between devices and explain. The operating system already knows
 * whether it is night — asking a second time would be the application having
 * an opinion about something that is not its business (§8.9).
 *
 * # Why not Material You
 *
 * Dynamic colour would take the palette from the wallpaper, which is somebody
 * else's decision about what this application looks like, changing whenever
 * they change a picture. The purple is the one the surfaces were built and
 * looked at in.
 */
private val PURPLE = Color(0xFF6750A4)
private val PURPLE_LIGHT = Color(0xFFD0BCFF)

private val Dark = darkColorScheme(
    primary = PURPLE_LIGHT,
    // The ground a capture window sits on at 3am. Not black: an OLED-black
    // sheet against a lit keyboard is its own kind of glare, and the elevation
    // Material draws with disappears against it.
    background = Color(0xFF141218),
    surface = Color(0xFF141218),
    surfaceVariant = Color(0xFF2B2930),
    onPrimary = Color(0xFF381E72),
    onBackground = Color(0xFFE6E1E5),
    onSurface = Color(0xFFE6E1E5),
    onSurfaceVariant = Color(0xFFCAC4D0),
    secondaryContainer = Color(0xFF4A4458),
    onSecondaryContainer = Color(0xFFE8DEF8),
    outline = Color(0xFF938F99),
)

private val Light = lightColorScheme(
    primary = PURPLE,
    background = Color(0xFFFFFBFE),
    surface = Color(0xFFFFFBFE),
    onBackground = Color(0xFF1C1B1F),
    onSurface = Color(0xFF1C1B1F),
    outline = Color(0xFF79747E),
)

/**
 * Wrap a surface in the palette.
 *
 * Used by every activity rather than each calling `MaterialTheme` itself —
 * two activities choosing their own colours is two applications that look
 * alike until one of them is changed.
 */
@Composable
fun CaptureTheme(content: @Composable () -> Unit) {
    MaterialTheme(
        colorScheme = if (isSystemInDarkTheme()) Dark else Light,
        content = content,
    )
}
