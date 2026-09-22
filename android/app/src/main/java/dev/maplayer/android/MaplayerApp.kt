package dev.maplayer.android

import android.app.Application
import computer.iroh.IrohAndroid

class MaplayerApp : Application() {
    lateinit var client: net.IrohClient
        private set

    override fun onCreate() {
        super.onCreate()
        IrohAndroid.installAndroidContext(this)
        client = net.IrohClient(this)
    }
}
