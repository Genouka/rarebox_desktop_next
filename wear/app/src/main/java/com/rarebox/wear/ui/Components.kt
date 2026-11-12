package com.rarebox.wear.ui

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.calculateEndPadding
import androidx.compose.foundation.layout.calculateStartPadding
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Popup
import androidx.wear.compose.foundation.lazy.TransformingLazyColumn
import androidx.wear.compose.foundation.lazy.TransformingLazyColumnItemScope
import androidx.wear.compose.foundation.lazy.TransformingLazyColumnScope
import androidx.wear.compose.foundation.lazy.rememberTransformingLazyColumnState
import androidx.wear.compose.material3.Button
import androidx.wear.compose.material3.ButtonDefaults
import androidx.wear.compose.material3.AppScaffold
import androidx.wear.compose.material3.Card
import androidx.wear.compose.material3.CardDefaults
import androidx.wear.compose.material3.Icon
import androidx.wear.compose.material3.IconButton
import androidx.wear.compose.material3.ListHeader
import androidx.wear.compose.material3.MaterialTheme
import androidx.wear.compose.material3.ScreenScaffold
import androidx.wear.compose.material3.SurfaceTransformation
import androidx.wear.compose.material3.Text
import androidx.wear.compose.material3.ripple
import com.rarebox.wear.R
import androidx.wear.compose.material3.lazy.TransformationSpec
import androidx.wear.compose.material3.lazy.rememberTransformationSpec
import androidx.wear.compose.material3.lazy.transformedHeight
import kotlinx.coroutines.delay

@Composable
fun RareBoxTheme(content: @Composable () -> Unit) {
    MaterialTheme {
        // 全应用唯一的 AppScaffold：TimeText 在页面切换时保持可见。
        AppScaffold(modifier = Modifier.fillMaxSize().background(MaterialTheme.colorScheme.background)) {
            content()
        }
    }
}

/** 页面级形变参数；由 [WearScreen] 提供，条目用它做 `transformedHeight`。 */
val LocalTransformationSpec = staticCompositionLocalOf<TransformationSpec> {
    error("LocalTransformationSpec 必须在 WearScreen 内使用")
}

/**
 * 所有页面的统一骨架：曲度滚动、进入/退出形变、边缘回弹与滚动指示器。
 *
 * 内容按 `item { ... }` 组织，条目内用 [WearCard] / [WearButton] / [WearRow] 等
 * 扩展，它们会自动叠加形变。页面必须有标题，子页传 [onBack]。
 */
@Composable
fun WearScreen(
    title: String,
    onBack: (() -> Unit)? = null,
    actions: (@Composable androidx.compose.foundation.layout.RowScope.() -> Unit)? = null,
    state: androidx.wear.compose.foundation.lazy.TransformingLazyColumnState =
        rememberTransformingLazyColumnState(),
    content: TransformingLazyColumnScope.() -> Unit,
) {
    val spec = rememberTransformationSpec()
    val screenHeight = androidx.compose.ui.platform.LocalConfiguration.current.screenHeightDp.dp
    ScreenScaffold(scrollState = state) { padding ->
        CompositionLocalProvider(LocalTransformationSpec provides spec) {
            TransformingLazyColumn(
                state = state,
                contentPadding = PaddingValues(
                    start = padding.calculateStartPadding(androidx.compose.ui.platform.LocalLayoutDirection.current),
                    end = padding.calculateEndPadding(androidx.compose.ui.platform.LocalLayoutDirection.current),
                    top = padding.calculateTopPadding() + screenHeight * 0.06f,
                    bottom = padding.calculateBottomPadding() + screenHeight * 0.30f,
                ),
                modifier = Modifier.fillMaxSize(),
            ) {
                item {
                    ScreenHeader(title = title, onBack = onBack, actions = actions)
                }
                content()
                item {
                    Spacer(Modifier.transformedHeight(this, spec).height(24.dp))
                }
            }
        }
    }
}

/** 标题行：时间 + 返回图标 + 标题。返回用 IconButton，自带涟漪。 */
@Composable
fun TransformingLazyColumnItemScope.ScreenHeader(
    title: String,
    onBack: (() -> Unit)? = null,
    actions: (@Composable androidx.compose.foundation.layout.RowScope.() -> Unit)? = null,
) {
    val spec = LocalTransformationSpec.current
    val modifier = Modifier
        .fillMaxWidth()
        .transformedHeight(this, spec)
    Column(modifier = modifier) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            if (onBack != null) {
                IconButton(onClick = onBack, modifier = Modifier.size(36.dp)) {
                    Icon(
                        painter = painterResource(R.drawable.ic_back),
                        contentDescription = "返回",
                        tint = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.size(18.dp),
                    )
                }
            } else {
                Spacer(Modifier.size(36.dp))
            }
            Spacer(Modifier.weight(1f))
            if (actions != null) {
                Row(
                    horizontalArrangement = Arrangement.spacedBy(2.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    content = actions,
                )
            }
        }
        ListHeader(transformation = SurfaceTransformation(spec)) {
            Text(
                text = title,
                style = MaterialTheme.typography.titleMedium,
                textAlign = TextAlign.Center,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.fillMaxWidth(),
            )
        }
    }
}

/** 整行按钮（图标 + 文案），带形变与涟漪。 */
@Composable
fun TransformingLazyColumnItemScope.WearButton(
    icon: Int? = null,
    label: String,
    enabled: Boolean = true,
    onClick: () -> Unit,
) {
    val spec = LocalTransformationSpec.current
    val iconSlot: (@androidx.compose.runtime.Composable androidx.compose.foundation.layout.BoxScope.() -> Unit)? =
        if (icon != null) {
            {
                Icon(
                    painter = painterResource(icon),
                    contentDescription = null,
                    modifier = Modifier.size(18.dp),
                )
            }
        } else {
            null
        }
    Button(
        onClick = onClick,
        enabled = enabled,
        modifier = Modifier
            .fillMaxWidth()
            .transformedHeight(this, spec)
            .minimumVerticalContentPadding(ButtonDefaults.minimumVerticalListContentPadding),
        transformation = SurfaceTransformation(spec),
        icon = iconSlot,
    ) {
        Text(label, maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}

/** 卡片容器；[onClick] 非空时整卡可点（带涟漪）。 */
@Composable
fun TransformingLazyColumnItemScope.WearCard(
    title: String? = null,
    onClick: (() -> Unit)? = null,
    content: @Composable ColumnScope.() -> Unit,
) {
    val spec = LocalTransformationSpec.current
    val modifier = Modifier
        .fillMaxWidth()
        .transformedHeight(this, spec)
        .minimumVerticalContentPadding(CardDefaults.minimumVerticalListContentPadding)
    if (onClick != null) {
        Card(
            onClick = onClick,
            modifier = modifier,
            transformation = SurfaceTransformation(spec),
        ) {
            if (title != null) {
                Text(
                    title,
                    style = MaterialTheme.typography.labelMedium,
                    color = MaterialTheme.colorScheme.primary,
                )
            }
            content()
        }
    } else {
        Card(modifier = modifier, transformation = SurfaceTransformation(spec)) {
            if (title != null) {
                Text(
                    title,
                    style = MaterialTheme.typography.labelMedium,
                    color = MaterialTheme.colorScheme.primary,
                )
            }
            content()
        }
    }
}

/** 列表行：图标 + 主文案 + 右侧值。 */
@Composable
fun TransformingLazyColumnItemScope.WearRow(
    icon: Int? = null,
    label: String,
    value: String? = null,
    enabled: Boolean = true,
    onClick: (() -> Unit)? = null,
) {
    val colors = MaterialTheme.colorScheme
    val modifier = Modifier.fillMaxWidth()
    val body: @Composable ColumnScope.() -> Unit = {
        Row(
            modifier = Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            if (icon != null) {
                Icon(
                    painter = painterResource(icon),
                    contentDescription = null,
                    tint = colors.onSurfaceVariant,
                    modifier = Modifier.size(18.dp),
                )
            }
            Text(
                text = label,
                style = MaterialTheme.typography.bodyLarge,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f),
            )
            if (value != null) {
                Text(
                    text = value,
                    style = MaterialTheme.typography.bodyMedium,
                    color = if (enabled) colors.onSurfaceVariant else colors.outline,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
    }
    if (onClick != null) {
        WearCard(onClick = onClick) { body() }
    } else {
        WearCard { body() }
    }
}

/**
 * 图标方块：2 列网格用。`onClick` 必填，整块可点并带涟漪。
 * 长按显示名称 tooltip；纯图标场景下把 [showLabel] 设为 false。
 */
@Composable
fun IconTile(
    icon: Int,
    label: String,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    showLabel: Boolean = true,
    onClick: () -> Unit,
) {
    val colors = MaterialTheme.colorScheme
    Card(
        onClick = onClick,
        enabled = enabled,
        modifier = modifier.height(if (showLabel) 76.dp else 56.dp),
        shape = RoundedCornerShape(20.dp),
        colors = CardDefaults.cardColors(
            containerColor = if (enabled) colors.surfaceContainerHigh else colors.surfaceContainer,
        ),
    ) {
        Column(
            modifier = Modifier.fillMaxWidth().padding(vertical = 10.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            Icon(
                painter = painterResource(icon),
                contentDescription = label,
                tint = if (enabled) colors.primary else colors.outline,
                modifier = Modifier.size(24.dp),
            )
            if (showLabel) {
                Text(
                    text = label,
                    style = MaterialTheme.typography.labelSmall,
                    color = if (enabled) colors.onSurface else colors.outline,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
    }
}

/**
 * 纯图标按钮 + 长按 tooltip。
 *
 * Wear Material3 1.6 没有 Tooltip 组件，这里用外层 `pointerInput` 观察长按
 * （不消费事件，不影响按钮点击），再用 `Popup` 显示名称。
 */
@OptIn(ExperimentalFoundationApi::class)
@Composable
fun IconAction(
    icon: Int,
    label: String,
    enabled: Boolean = true,
    onClick: () -> Unit,
) {
    val colors = MaterialTheme.colorScheme
    var tooltip by remember { mutableStateOf(false) }
    val interaction = remember { MutableInteractionSource() }
    Box(
        modifier = Modifier
            .size(44.dp)
            .clip(CircleShape)
            .combinedClickable(
                interactionSource = interaction,
                indication = ripple(),
                enabled = enabled,
                onClick = onClick,
                onLongClick = { tooltip = true },
            ),
        contentAlignment = Alignment.Center,
    ) {
        Icon(
            painter = painterResource(icon),
            contentDescription = label,
            tint = if (enabled) colors.onSurfaceVariant else colors.outline,
            modifier = Modifier.size(20.dp),
        )
        if (tooltip) {
            LaunchedEffect(tooltip) {
                delay(1600)
                tooltip = false
            }
            Popup(
                alignment = Alignment.TopCenter,
                offset = IntOffset(0, -52),
            ) {
                Card {
                    Text(
                        text = label,
                        style = MaterialTheme.typography.labelSmall,
                        color = colors.onSurface,
                        maxLines = 1,
                    )
                }
            }
        }
    }
}

@Composable
fun InfoLine(label: String, value: String?) {
    if (value.isNullOrBlank()) return
    Row(
        modifier = Modifier.fillMaxWidth().padding(vertical = 1.dp),
        horizontalArrangement = Arrangement.SpaceBetween,
    ) {
        Text(
            label,
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Text(
            value,
            style = MaterialTheme.typography.bodySmall,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
    }
}

@Composable
fun MonoText(text: String, maxLines: Int = 6, color: Color = MaterialTheme.colorScheme.onSurface) {
    Text(
        text = text,
        style = MaterialTheme.typography.bodySmall,
        fontFamily = FontFamily.Monospace,
        maxLines = maxLines,
        color = color,
        modifier = Modifier.fillMaxWidth(),
    )
}

/** 无外框输入行：仍是 BasicTextField，但外观跟随主题。 */
@Composable
fun SimpleTextField(
    value: String,
    placeholder: String,
    onValueChange: (String) -> Unit,
    singleLine: Boolean = true,
) {
    androidx.compose.foundation.text.BasicTextField(
        value = value,
        onValueChange = onValueChange,
        singleLine = singleLine,
        textStyle = androidx.compose.ui.text.TextStyle(
            color = MaterialTheme.colorScheme.onSurface,
            fontSize = 14.sp,
        ),
        modifier = Modifier
            .fillMaxWidth()
            .background(MaterialTheme.colorScheme.surfaceContainerHigh, RoundedCornerShape(10.dp))
            .padding(horizontal = 12.dp, vertical = 9.dp),
        decorationBox = { inner ->
            Box {
                if (value.isEmpty()) {
                    Text(
                        placeholder,
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                inner()
            }
        },
    )
}

/** 两列图标网格；作为单个列表条目使用。 */
@Composable
fun TransformingLazyColumnItemScope.IconGrid(
    entries: List<Triple<Int, String, () -> Unit>>,
    columns: Int = 2,
) {
    val spec = LocalTransformationSpec.current
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .transformedHeight(this, spec),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        entries.chunked(columns).forEach { row ->
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                row.forEach { (icon, label, action) ->
                    IconTile(
                        icon = icon,
                        label = label,
                        modifier = Modifier.weight(1f),
                        onClick = action,
                    )
                }
                repeat(columns - row.size) { Spacer(Modifier.weight(1f)) }
            }
        }
    }
}

/** 单行图标块（2~3 个），与 [IconGrid] 一样作为单个列表条目。 */
@Composable
fun TransformingLazyColumnItemScope.IconRow(
    entries: List<Triple<Int, String, () -> Unit>>,
) {
    val spec = LocalTransformationSpec.current
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .transformedHeight(this, spec),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        entries.forEach { (icon, label, action) ->
            IconTile(
                icon = icon,
                label = label,
                modifier = Modifier.weight(1f),
                onClick = action,
            )
        }
    }
}


/** 分区标题：用 ListHeader，而不是把标题塞进可点卡片里。 */
@Composable
fun TransformingLazyColumnItemScope.SectionHeader(
    title: String,
    subtitle: String? = null,
) {
    val spec = LocalTransformationSpec.current
    ListHeader(
        modifier = Modifier
            .fillMaxWidth()
            .transformedHeight(this, spec),
        transformation = SurfaceTransformation(spec),
    ) {
        Column(horizontalAlignment = Alignment.CenterHorizontally) {
            Text(text = title, style = MaterialTheme.typography.titleSmall)
            if (subtitle != null) {
                Text(
                    text = subtitle,
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

/** 次要动作：紧凑按钮，避免整页都是大号 Button。 */
@Composable
fun TransformingLazyColumnItemScope.WearCompact(
    label: String,
    icon: Int? = null,
    enabled: Boolean = true,
    onClick: () -> Unit,
) {
    val spec = LocalTransformationSpec.current
    val iconSlot: (@androidx.compose.runtime.Composable androidx.compose.foundation.layout.BoxScope.() -> Unit)? =
        if (icon != null) {
            {
                Icon(
                    painter = painterResource(icon),
                    contentDescription = null,
                    modifier = Modifier.size(16.dp),
                )
            }
        } else {
            null
        }
    androidx.wear.compose.material3.CompactButton(
        onClick = onClick,
        enabled = enabled,
        modifier = Modifier
            .fillMaxWidth()
            .transformedHeight(this, spec)
            .minimumVerticalContentPadding(
                androidx.wear.compose.material3.CompactButtonDefaults.minimumVerticalListContentPadding,
            ),
        transformation = SurfaceTransformation(spec),
        icon = iconSlot,
    ) {
        Text(label, maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}
