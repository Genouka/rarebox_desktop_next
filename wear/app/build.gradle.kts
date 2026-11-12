import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
    id("org.jetbrains.kotlin.plugin.serialization")
}

val repoRoot = rootProject.projectDir.parentFile

android {
    namespace = "com.rarebox.wear"
    compileSdk = 35

    defaultConfig {
        applicationId = "com.rarebox.wear"
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
    }

    sourceSets["main"].kotlin.srcDirs("${repoRoot}/bindings/kotlin")
    sourceSets["main"].jniLibs.srcDirs("${repoRoot}/dist/wear-jniLibs")

    buildTypes {
        release {
            isMinifyEnabled = false
            // 侧载测试默认使用 debug 签名；正式分发可通过 gradle.properties 注入自己的密钥。
            signingConfig = signingConfigs.getByName("debug")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }

    packaging {
        jniLibs {
            // libadb.so / libhdc.so 需要从 nativeLibraryDir 直接执行，必须解压到磁盘。
            useLegacyPackaging = true
        }
        resources {
            excludes += "/META-INF/{AL2.0,LGPL2.1}"
            excludes += "META-INF/versions/**/OSGI-INF/MANIFEST.MF"
        }
    }

    lint {
        abortOnError = false
        checkReleaseBuilds = false
    }
}

kotlin {
    compilerOptions {
        jvmTarget.set(JvmTarget.JVM_17)
    }
}

dependencies {
    implementation(platform("androidx.compose:compose-bom:2025.12.00"))
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.foundation:foundation")
    implementation("androidx.wear.compose:compose-foundation:1.6.2")
    implementation("androidx.wear.compose:compose-material3:1.6.2")
    implementation("androidx.activity:activity-compose:1.9.2")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.8.6")
    implementation("androidx.lifecycle:lifecycle-runtime-ktx:2.8.6")
    implementation("androidx.core:core-ktx:1.13.1")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.9.0")
    implementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.7.3")
}
