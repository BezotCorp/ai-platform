package io.github.bezotcorp.bcaip.providers.openai

public fun provider(
    apiKey: String,
    baseUrl: String? = null,
): io.github.bezotcorp.bcaip.Provider = io.github.bezotcorp.bcaip.openaiProvider(apiKey, baseUrl)

public fun defaultModel(): String = io.github.bezotcorp.bcaip.openaiDefaultModel()
