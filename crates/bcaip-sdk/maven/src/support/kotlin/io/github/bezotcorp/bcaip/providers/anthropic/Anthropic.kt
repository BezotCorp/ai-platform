package io.github.bezotcorp.bcaip.providers.anthropic

public fun provider(
    apiKey: String,
    baseUrl: String? = null,
    betaHeaders: List<String> = emptyList(),
): io.github.bezotcorp.bcaip.Provider = io.github.bezotcorp.bcaip.anthropicProvider(apiKey, baseUrl, betaHeaders)

public fun defaultModel(): String = io.github.bezotcorp.bcaip.anthropicDefaultModel()
