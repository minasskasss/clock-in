import groovy.json.JsonSlurper

plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

// Generated at build time from the repo's single sources (never committed):
// res/raw/check_in.wav, check_out.wav from assets/sounds/, and
// res/values/strings.xml from the "alarm" and "android" sections of
// src/i18n/el.json (CLAUDE.md: every user-facing string lives in el.json).
val repoRoot = projectDir.resolve("../../..").canonicalFile
val generatedRes: File = layout.buildDirectory.dir("generated/clockin-res").get().asFile

val generateClockinResources = tasks.register("generateClockinResources") {
    val elJson = repoRoot.resolve("src/i18n/el.json")
    val sounds = repoRoot.resolve("assets/sounds")
    inputs.file(elJson)
    inputs.dir(sounds)
    outputs.dir(generatedRes)
    val out = generatedRes
    doLast {
        val raw = out.resolve("raw").apply { mkdirs() }
        sounds.resolve("check-in.wav").copyTo(raw.resolve("check_in.wav"), overwrite = true)
        sounds.resolve("check-out.wav").copyTo(raw.resolve("check_out.wav"), overwrite = true)

        @Suppress("UNCHECKED_CAST")
        val strings = JsonSlurper().parse(elJson, "UTF-8") as Map<String, Any?>
        val xml = StringBuilder("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<!-- Generated from src/i18n/el.json. Do not edit. -->\n<resources>\n")
        for (section in listOf("alarm", "android")) {
            @Suppress("UNCHECKED_CAST")
            val entries = strings[section] as? Map<String, Any?> ?: error("el.json has no \"$section\" section")
            for ((key, value) in entries) {
                if (value !is String) continue
                // i18next placeholders become positional Android ones, in order.
                var index = 0
                val text = Regex("\\{\\{\\s*\\w+\\s*\\}\\}").replace(value) { "%${++index}\$s" }
                val escaped = text
                    .replace("\\", "\\\\")
                    .replace("&", "&amp;")
                    .replace("<", "&lt;")
                    .replace(">", "&gt;")
                    .replace("\"", "\\\"")
                    .replace("'", "\\'")
                    .replace("\n", "\\n")
                    .let { if (it.startsWith("@") || it.startsWith("?")) "\\$it" else it }
                val formatted = if (index == 0) " formatted=\"false\"" else ""
                xml.append("    <string name=\"clockin_${section}_$key\"$formatted>\"$escaped\"</string>\n")
            }
        }
        xml.append("</resources>\n")
        out.resolve("values").apply { mkdirs() }.resolve("strings.xml").writeText(xml.toString(), Charsets.UTF_8)
    }
}

android {
    namespace = "io.github.minasskasss.clockin.alarm"
    compileSdk = 37

    defaultConfig {
        minSdk = 26
        consumerProguardFiles("consumer-rules.pro")
    }

    sourceSets {
        getByName("main") {
            res.srcDir(generatedRes)
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_1_8
        targetCompatibility = JavaVersion.VERSION_1_8
    }
}

kotlin {
    compilerOptions {
        jvmTarget = org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_1_8
    }
}

tasks.named("preBuild") { dependsOn(generateClockinResources) }

dependencies {
    implementation("androidx.core:core-ktx:1.16.0")
    implementation("androidx.appcompat:appcompat:1.7.1")
    implementation("androidx.work:work-runtime-ktx:2.10.3")
    implementation(project(":tauri-android"))
}
