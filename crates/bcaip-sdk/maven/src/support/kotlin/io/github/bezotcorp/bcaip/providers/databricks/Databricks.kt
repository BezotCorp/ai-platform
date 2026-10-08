package io.github.bezotcorp.bcaip.providers.databricks

public fun provider(host: String, token: String): io.github.bezotcorp.bcaip.Provider =
    io.github.bezotcorp.bcaip.databricksProvider(host, token)

public fun defaultModel(): String = io.github.bezotcorp.bcaip.databricksDefaultModel()
