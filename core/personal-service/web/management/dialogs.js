// Use DOM dialogs because embedded Android WebViews may not provide native JS dialogs.
let dialogSequence = 0

function showDialog(message, initialValue, sensitive = false) {
    return new Promise((resolve) => {
        const previousFocus = document.activeElement
        const dialog = document.createElement("dialog")
        dialog.className = "management-dialog"
        const form = document.createElement("form")
        form.className = "stack-form"
        const messageNode = document.createElement(initialValue === undefined ? "p" : "label")
        messageNode.className = "dialog-message"
        messageNode.textContent = message
        messageNode.id = `management-dialog-${++dialogSequence}`
        dialog.setAttribute("aria-labelledby", messageNode.id)
        form.append(messageNode)
        let input
        if (initialValue !== undefined) {
            input = document.createElement("input")
            input.type = sensitive ? "password" : "text"
            input.value = initialValue
            input.maxLength = 4096
            input.autocomplete = "off"
            input.setAttribute("aria-labelledby", messageNode.id)
            form.append(input)
        }
        const controls = document.createElement("div")
        controls.className = "dialog-actions"
        const cancel = document.createElement("button")
        cancel.type = "button"; cancel.className = "button ghost"; cancel.textContent = "取消"
        const accept = document.createElement("button")
        accept.type = "submit"; accept.className = "button primary"; accept.textContent = "确认"
        controls.append(cancel, accept); form.append(controls); dialog.append(form)
        let finished = false
        const finish = (accepted) => {
            if (finished) return
            finished = true
            const value = input ? (accepted ? input.value : null) : accepted
            dialog.close(); dialog.remove()
            if (previousFocus?.isConnected) previousFocus.focus()
            resolve(value)
        }
        form.addEventListener("submit", (event) => { event.preventDefault(); finish(true) })
        cancel.addEventListener("click", () => finish(false))
        dialog.addEventListener("cancel", (event) => { event.preventDefault(); finish(false) })
        dialog.addEventListener("close", () => finish(false))
        document.body.append(dialog)
        dialog.showModal()
        if (input) { input.focus(); input.select() } else cancel.focus()
    })
}

export function confirmAction(message) { return showDialog(message) }
export function promptInput(message, initialValue = "", { sensitive = false } = {}) {
    return showDialog(message, initialValue, sensitive)
}
