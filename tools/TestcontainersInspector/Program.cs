using System;
using System.Linq;
using DotNet.Testcontainers.Configurations;
using DotNet.Testcontainers.Containers;

static void DumpType(string title, Type type)
{
	Console.WriteLine(title);
	Console.WriteLine(new string('-', title.Length));

	foreach (var method in type.GetMethods().OrderBy(m => m.Name))
	{
		var parameters = string.Join(", ", method.GetParameters().Select(p => $"{p.ParameterType.Name} {p.Name}"));
		Console.WriteLine($"{method.ReturnType.Name} {method.Name}({parameters})");
	}

	Console.WriteLine();
	foreach (var property in type.GetProperties().OrderBy(p => p.Name))
	{
		Console.WriteLine($"Property: {property.PropertyType.Name} {property.Name} (CanWrite: {property.CanWrite})");
	}
}

DumpType("ITestcontainersContainer", typeof(ITestcontainersContainer));
Console.WriteLine();
DumpType("MsSqlTestcontainerConfiguration", typeof(MsSqlTestcontainerConfiguration));
