package io.github.bezotcorp.bcaip.providers.groq

public fun provider(apiKey: String): io.github.bezotcorp.bcaip.Provider = io.github.bezotcorp.bcaip.groqProvider(apiKey)

public fun defaultModel(): String = io.github.bezotcorp.bcaip.groqDefaultModel()
