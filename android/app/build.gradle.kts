plugins {
    id("com.android.application")
}

android {
    namespace = "com.rarebox.desktop"
    compileSdk = 35

    defaultConfig {
        applicationId = "com.rarebox.desktop"
        minSdk = 26
        targetSdk = 34
        versionCode = 1
        versionName = "0.1.0"

        ndk {
            abiFilters += (findProperty("rarebox.abis") as? String ?: "arm64-v8a,x86_64")
                .split(',')
                .map { it.trim() }
                .filter { it.isNotEmpty() }
        }

        manifestPlaceholders["nativeLibraryName"] = "rarebox_desktop"
    }

    signingConfigs {
        val storePath = findProperty("rarebox.keystore") as String?
        if (storePath != null) {
            create("release") {
                storeFile = file(storePath)
                storePassword = findProperty("rarebox.keystore.password") as String?
                keyAlias = findProperty("rarebox.key.alias") as String?
                keyPassword = findProperty("rarebox.key.password") as String?
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            // 默认用 debug 签名，便于直接侧载测试；正式分发时通过
            // gradle.properties / -P 传入 rarebox.keystore* 使用自己的密钥。
            signingConfig = if (findProperty("rarebox.keystore") != null) {
                signingConfigs.getByName("release")
            } else {
                signingConfigs.getByName("debug")
            }
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro",
            )
        }
        debug {
            isDebuggable = true
            isJniDebuggable = true
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    packaging {
        jniLibs {
            // 内置 adb/hdc 需要从 nativeLibraryDir 直接执行，必须让安装时把 .so 解压到
            // 磁盘（AGP 默认对 targetSdk >= 23 使用压缩在 APK 内、不落盘的布局）。
            useLegacyPackaging = true
            keepDebugSymbols += listOf(
                "*/arm64-v8a/librarebox_desktop.so",
                "*/armeabi-v7a/librarebox_desktop.so",
                "*/x86_64/librarebox_desktop.so",
            )
        }
    }

    lint {
        abortOnError = false
        checkReleaseBuilds = false
    }
}
