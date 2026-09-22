package dev.maplayer.android

import android.app.Application
import computer.iroh.IrohAndroid
import dev.maplayer.android.net.IrohClient

class MaplayerApp : Application() {
    lateinit var client: IrohClient
        private set

    override fun onCreate() {
        super.onCreate()
        IrohAndroid.installAndroidContext(this)
        client = IrohClient(this)
    }
}
