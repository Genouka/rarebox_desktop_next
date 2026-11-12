package com.rarebox.wear.ui

/** 单 Activity 的返回栈；Wear 的系统返回手势直接走 BackHandler。 */
sealed interface Route {
    data object Home : Route
    data object More : Route

    // 连接：Hub + 子页
    data object Connect : Route
    data object ConnectWireless : Route
    data object ConnectPair : Route
    data object ConnectWlan : Route
    data object ConnectUsb : Route
    data object ConnectScan : Route
    data object ConnectDevices : Route

    data object Overview : Route
    data object Files : Route
    data object Apps : Route
    data class AppDetail(val name: String, val label: String) : Route

    // 屏幕：Hub + 子页
    data object Display : Route
    data object Screenshot : Route
    data object DisplayParams : Route

    data object Remote : Route
    data object Terminal : Route
    data object Logs : Route

    // 设置：Hub + 子页
    data object Settings : Route
    data object SettingsAbout : Route
    data object SettingsTools : Route
    data object SettingsLegal : Route
}
