package com.arnis.app

object NativeBridge {
    init {
        System.loadLibrary("arnis_mobile")
    }

    external fun generateWorld(
        minLat: Double,
        minLon: Double,
        maxLat: Double,
        maxLon: Double,
        isBedrock: Boolean,
        version: String,
        outputPath: String
    ): Boolean

    external fun cancelGeneration()
}
