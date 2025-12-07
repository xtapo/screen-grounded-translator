Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes

$root = [System.Windows.Automation.AutomationElement]::RootElement
$condition = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ClassNameProperty, "LiveCaptionsDesktopWindow")
$lcWindow = $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $condition)

if ($lcWindow) {
    Write-Output "FOUND"
    
    $textCondition = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::AutomationIdProperty, "CaptionsTextBlock")
    $textElement = $lcWindow.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $textCondition)
    
    if ($textElement) {
        Write-Output ("TEXT_ELEMENT_FOUND: " + $textElement.Current.Name)
    } else {
        Write-Output "TEXT_ELEMENT_NOT_FOUND"
    }
} else {
    Write-Output "NOT_FOUND"
}
